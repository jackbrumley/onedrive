use crate::app::account_profiles::{load_profiles, save_profiles, AccountProfile};
use crate::app::activity_log;
use crate::app::auth::{load_auth_session, refresh_access_token};
use crate::app::log_context;
use crate::app::state::AppState;
use crate::app::sync_runtime::{self, SyncRuntimeMap};
use futures_util::{stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 8;
const MAX_DOWNLOAD_CONCURRENCY: usize = 32;
const MAX_DOWNLOAD_RETRIES: u32 = 5;
const MAX_RETRY_DELAY_SECONDS: u64 = 30;
const SYNC_CANCELLED_ERROR: &str = "Synchronization cancelled";
const CANCEL_POLL_INTERVAL_MILLIS: u64 = 100;

pub fn on_agent_state_changed(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
    agent_state: &str,
) -> Result<(), String> {
    log::info!(
        "{} SYNC_AGENT_STATE_CHANGE requested_state={}",
        log_context::account_prefix(profile_id),
        agent_state
    );
    if agent_state == "syncing" {
        let _ = set_cancel_flag(state, profile_id, false)?;
        runtime_clear_issue(&state.sync_runtime, profile_id);
        start_sync_worker(state, profile_id)?;
    } else {
        let _ = set_cancel_flag(state, profile_id, true)?;
        stop_sync_worker(state, profile_id)?;
        if let Ok(mut runtime_map) = state.sync_runtime.lock() {
            sync_runtime::clear_in_progress(&mut runtime_map, profile_id);
            sync_runtime::clear_issue(&mut runtime_map, profile_id);
            let (phase, message) = if agent_state == "paused" {
                ("paused", "Synchronization paused")
            } else {
                ("idle", "Idle")
            };
            sync_runtime::set_phase(&mut runtime_map, profile_id, phase, message);
        }
    }
    Ok(())
}

fn start_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let account_prefix = log_context::account_prefix(profile_id);
    let cancel_flag = set_cancel_flag(state, profile_id, false)?;
    let cycle_lock = get_or_create_cycle_lock(state, profile_id)?;
    let initial_delay = remaining_until_next_cycle(profile_id, Duration::from_secs(15));
    {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        if stops.contains_key(profile_id) {
            log::info!("{} SYNC_WORKER_ALREADY_RUNNING", account_prefix);
            return Ok(());
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        stops.insert(profile_id.to_string(), tx);
        log::info!(
            "{} SYNC_WORKER_STARTING interval_seconds=15",
            account_prefix
        );

        let profile_id_owned = profile_id.to_string();
        let profiles_lock = Arc::clone(&state.profiles_lock);
        let sync_runtime = Arc::clone(&state.sync_runtime);
        if let Ok(mut runtime_map) = sync_runtime.lock() {
            sync_runtime::set_phase(
                &mut runtime_map,
                &profile_id_owned,
                "syncing",
                "Preparing next sync cycle",
            );
            sync_runtime::clear_issue(&mut runtime_map, &profile_id_owned);
        }
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            if let Some(delay) = initial_delay {
                log::info!(
                    "{} SYNC_WORKER_RESUME_DELAY wait_ms={}",
                    log_context::account_prefix(&profile_id_owned),
                    delay.as_millis()
                );
                if sleep_with_cancellation(&cancel_flag, delay).await.is_err() {
                    if let Ok(mut runtime_map) = sync_runtime.lock() {
                        sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                        sync_runtime::set_phase(
                            &mut runtime_map,
                            &profile_id_owned,
                            "paused",
                            "Synchronization paused",
                        );
                    }
                    return;
                }
            }
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        log::info!("{} SYNC_WORKER_STOP_SIGNAL", log_context::account_prefix(&profile_id_owned));
                        if let Ok(mut runtime_map) = sync_runtime.lock() {
                            sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                            sync_runtime::set_phase(&mut runtime_map, &profile_id_owned, "paused", "Synchronization paused");
                        }
                        break;
                    }
                    _ = ticker.tick() => {
                        if cancel_flag.load(Ordering::Relaxed) {
                            continue;
                        }
                        let _cycle_guard = match cycle_lock.try_lock() {
                            Ok(guard) => guard,
                            Err(_) => {
                                log::warn!(
                                    "{} SYNC_TICK_SKIPPED cycle already running",
                                    log_context::account_prefix(&profile_id_owned)
                                );
                                continue;
                            }
                        };
                        log::info!("{} SYNC_TICK", log_context::account_prefix(&profile_id_owned));
                        match tick_sync_cycle(&profiles_lock, &sync_runtime, &profile_id_owned, &cancel_flag).await {
                            Ok(stats) => {
                                log::info!(
                                    "{} [cycle:{}] SYNC_CYCLE_COMPLETE downloaded={} uploaded={} local_deleted={} remote_deleted={} remote_folders={} remote_pages={} remote_items={} local_items={}",
                                    stats.account_prefix,
                                    stats.cycle_id,
                                    stats.downloaded_files,
                                    stats.uploaded_files,
                                    stats.deleted_local,
                                    stats.deleted_remote,
                                    stats.created_remote_folders,
                                    stats.remote_pages,
                                    stats.remote_items_received,
                                    stats.local_items_seen,
                                );
                            }
                            Err(error) => {
                                if is_sync_cancelled_error(&error) {
                                    log::info!(
                                        "{} SYNC_CYCLE_CANCELLED",
                                        log_context::account_prefix(&profile_id_owned)
                                    );
                                    if let Ok(mut runtime_map) = sync_runtime.lock() {
                                        sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                                    }
                                    continue;
                                }
                                let (issue_code, issue_actions) = classify_sync_issue(&error);
                                log::error!(
                                    "{} SYNC_CYCLE_FAILED {}",
                                    log_context::account_prefix(&profile_id_owned),
                                    error
                                );
                                if let Ok(mut runtime_map) = sync_runtime.lock() {
                                    sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                                    sync_runtime::set_phase(
                                        &mut runtime_map,
                                        &profile_id_owned,
                                        "error",
                                        &format!("Sync error: {}", error),
                                    );
                                    sync_runtime::set_issue(
                                        &mut runtime_map,
                                        &profile_id_owned,
                                        issue_code,
                                        &error,
                                        issue_actions,
                                        None,
                                        None,
                                    );
                                }
                                let _ = activity_log::append_event(
                                    &profile_id_owned,
                                    &log_context::account_identity(&profile_id_owned),
                                    "error",
                                    &format!(
                                        "{} SYNC_CYCLE_FAILED {error}",
                                        log_context::account_prefix(&profile_id_owned)
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    let _ = activity_log::append_event(
        profile_id,
        &log_context::account_identity(profile_id),
        "info",
        &format!(
            "{} Sync agent started",
            log_context::account_prefix(profile_id)
        ),
    );
    Ok(())
}

fn stop_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let _ = set_cancel_flag(state, profile_id, true)?;
    let maybe_sender = {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        stops.remove(profile_id)
    };

    if let Some(sender) = maybe_sender {
        let _ = sender.send(());
        let _ = activity_log::append_event(
            profile_id,
            &log_context::account_identity(profile_id),
            "info",
            &format!(
                "{} Sync agent stopped",
                log_context::account_prefix(profile_id)
            ),
        );
    }

    Ok(())
}

#[derive(Default)]
struct SyncCycleStats {
    account_prefix: String,
    cycle_id: String,
    downloaded_files: usize,
    uploaded_files: usize,
    deleted_local: usize,
    deleted_remote: usize,
    created_remote_folders: usize,
    remote_pages: usize,
    remote_items_received: usize,
    local_items_seen: usize,
}

#[derive(Clone)]
struct GraphContext {
    profile_id: String,
    account_prefix: String,
    cycle_id: String,
    access_token: String,
    sync_runtime: Arc<std::sync::Mutex<SyncRuntimeMap>>,
}

impl GraphContext {
    async fn refresh_token(&mut self) -> Result<(), String> {
        let refreshed = refresh_access_token(&self.profile_id).await?;
        self.access_token = refreshed.access_token;
        Ok(())
    }
}

fn resolve_download_concurrency() -> usize {
    std::env::var("SOMEDRIVE_SYNC_DOWNLOAD_CONCURRENCY")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.clamp(1, MAX_DOWNLOAD_CONCURRENCY))
        .unwrap_or(DEFAULT_DOWNLOAD_CONCURRENCY)
}

fn parse_retry_after_delay(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let retry_after = headers.get(reqwest::header::RETRY_AFTER)?;
    let text = retry_after.to_str().ok()?.trim();
    if text.is_empty() {
        return None;
    }
    let seconds = text.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.min(MAX_RETRY_DELAY_SECONDS)))
}

fn exponential_backoff_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(6);
    let seconds = 2_u64.saturating_pow(exponent).min(MAX_RETRY_DELAY_SECONDS);
    Duration::from_secs(seconds)
}

fn is_sync_cancelled_error(error: &str) -> bool {
    error == SYNC_CANCELLED_ERROR
}

fn ensure_not_cancelled(cancel_flag: &Arc<AtomicBool>) -> Result<(), String> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(SYNC_CANCELLED_ERROR.to_string());
    }
    Ok(())
}

fn set_cancel_flag(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
    value: bool,
) -> Result<Arc<AtomicBool>, String> {
    let mut flags = state
        .sync_cancel_flags
        .lock()
        .map_err(|_| "Sync cancel flag lock is poisoned".to_string())?;
    let flag = flags
        .entry(profile_id.to_string())
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone();
    flag.store(value, Ordering::Relaxed);
    Ok(flag)
}

fn get_or_create_cycle_lock(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    let mut locks = state
        .sync_cycle_locks
        .lock()
        .map_err(|_| "Sync cycle lock map is poisoned".to_string())?;
    Ok(locks
        .entry(profile_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone())
}

async fn wait_for_cancellation(cancel_flag: Arc<AtomicBool>) {
    while !cancel_flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(CANCEL_POLL_INTERVAL_MILLIS)).await;
    }
}

async fn sleep_with_cancellation(
    cancel_flag: &Arc<AtomicBool>,
    duration: Duration,
) -> Result<(), String> {
    ensure_not_cancelled(cancel_flag)?;
    if duration.is_zero() {
        return Ok(());
    }
    tokio::select! {
        _ = wait_for_cancellation(Arc::clone(cancel_flag)) => Err(SYNC_CANCELLED_ERROR.to_string()),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

fn runtime_set_phase(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    phase: &str,
    phase_message: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_phase(&mut runtime_map, profile_id, phase, phase_message);
    }
}

fn runtime_clear_issue(runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>, profile_id: &str) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::clear_issue(&mut runtime_map, profile_id);
    }
}

fn classify_sync_issue(error: &str) -> (&'static str, &'static [&'static str]) {
    let normalized = error.to_lowercase();
    if normalized.contains("re-authentication")
        || normalized.contains("not authenticated")
        || normalized.contains("access token is empty")
        || normalized.contains("401")
    {
        return ("auth_required", &["reauthenticate", "retry_sync"]);
    }
    if normalized.contains("429") || normalized.contains("too many requests") {
        return ("rate_limited", &["retry_sync"]);
    }
    if normalized.contains("permission denied") || normalized.contains("403") {
        return ("permission_denied", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("no space left") || normalized.contains("disk full") {
        return ("disk_full", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("sync root") {
        return ("sync_root_unavailable", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("conflict") || normalized.contains("safebackup") {
        return (
            "conflict_detected",
            &["open_conflict", "open_sync_root", "retry_sync"],
        );
    }
    if normalized.contains("network")
        || normalized.contains("connection")
        || normalized.contains("timed out")
        || normalized.contains("dns")
    {
        return ("network_unavailable", &["retry_sync"]);
    }
    ("unknown_error", &["retry_sync"])
}

fn relative_path_for_issue(sync_root: &Path, candidate: &Path) -> Option<String> {
    let relative = candidate.strip_prefix(sync_root).ok()?;
    let mut output = String::new();
    for component in relative.components() {
        if let std::path::Component::Normal(segment) = component {
            if !output.is_empty() {
                output.push('/');
            }
            output.push_str(&segment.to_string_lossy());
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

fn runtime_start_transfer(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    direction: &str,
    path: &str,
    bytes_total: Option<u64>,
) -> Option<String> {
    let mut runtime_map = runtime.lock().ok()?;
    Some(sync_runtime::start_transfer(
        &mut runtime_map,
        profile_id,
        direction,
        path,
        bytes_total,
    ))
}

fn runtime_update_transfer_progress(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    transfer_id: &str,
    bytes_done: u64,
    bytes_total: Option<u64>,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::update_transfer_progress(
            &mut runtime_map,
            profile_id,
            transfer_id,
            bytes_done,
            bytes_total,
        );
    }
}

fn runtime_finish_transfer_success(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    transfer_id: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::finish_transfer_success(&mut runtime_map, profile_id, transfer_id);
    }
}

fn runtime_finish_transfer_error(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    transfer_id: &str,
    error: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::finish_transfer_error(&mut runtime_map, profile_id, transfer_id, error);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LocalSnapshotEntry {
    is_dir: bool,
    size: u64,
    modified_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteKnownItem {
    id: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified_ts: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSyncState {
    delta_link: Option<String>,
    remote_by_id: HashMap<String, RemoteKnownItem>,
    remote_path_to_id: HashMap<String, String>,
    local_snapshot: HashMap<String, LocalSnapshotEntry>,
    last_cycle_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeltaResponse {
    value: Vec<DeltaItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaItem {
    id: String,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    deleted: Option<serde_json::Value>,
    parent_reference: Option<ParentReference>,
    last_modified_date_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParentReference {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveItemResponse {
    id: String,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    parent_reference: Option<ParentReference>,
    last_modified_date_time: Option<String>,
}

async fn tick_sync_cycle(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    sync_runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<SyncCycleStats, String> {
    ensure_not_cancelled(cancel_flag)?;
    let profile = load_syncable_profile(profiles_lock, profile_id)?;
    let account_prefix = log_context::account_prefix_from_parts(profile_id, &profile.email);
    let cycle_id = new_cycle_id();
    let sync_root = PathBuf::from(profile.sync_root.clone());
    std::fs::create_dir_all(&sync_root).map_err(|error| {
        format!(
            "Failed to create sync root '{}': {}",
            sync_root.display(),
            error
        )
    })?;

    let session = load_auth_session(profile_id)?;
    if session.access_token.trim().is_empty() {
        return Err("Auth access token is empty; re-authentication required".to_string());
    }

    let mut graph = GraphContext {
        profile_id: profile_id.to_string(),
        account_prefix: account_prefix.clone(),
        cycle_id: cycle_id.clone(),
        access_token: session.access_token,
        sync_runtime: Arc::clone(sync_runtime),
    };

    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "syncing",
        "Preparing synchronization cycle",
    );

    let mut sync_state = load_sync_state(profile_id)?;
    let mut stats = SyncCycleStats {
        account_prefix: account_prefix.clone(),
        cycle_id: cycle_id.clone(),
        ..SyncCycleStats::default()
    };
    log::info!(
        "{} [cycle:{}] SYNC_CYCLE_START sync_root={}",
        account_prefix,
        cycle_id,
        sync_root.display()
    );
    let _ = activity_log::append_event(
        profile_id,
        &profile.email,
        "info",
        &format!("{} [cycle:{}] SYNC_CYCLE_START", account_prefix, cycle_id),
    );

    let remote_changes = fetch_delta_changes(
        &mut graph,
        sync_state.delta_link.clone(),
        &mut sync_state,
        &mut stats,
        cancel_flag,
    )
    .await?;

    ensure_not_cancelled(cancel_flag)?;
    apply_remote_changes(
        &mut graph,
        &sync_root,
        &remote_changes,
        &mut sync_state,
        &mut stats,
        cancel_flag,
    )
    .await?;

    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "scanning_local",
        "Scanning local files",
    );
    let local_snapshot = collect_local_snapshot(&sync_root)?;
    stats.local_items_seen = local_snapshot.len();
    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "applying_local",
        "Applying local changes",
    );
    ensure_not_cancelled(cancel_flag)?;
    apply_local_changes(
        &mut graph,
        &sync_root,
        &local_snapshot,
        &mut sync_state,
        &mut stats,
        cancel_flag,
    )
    .await?;

    sync_state.local_snapshot = collect_local_snapshot(&sync_root)?;
    sync_state.last_cycle_at = Some(chrono::Local::now().to_rfc3339());
    save_sync_state(profile_id, &sync_state)?;

    update_profile_last_sync(profiles_lock, profile_id)?;

    let summary = format!(
        "Sync cycle complete (downloaded {}, uploaded {}, remote deletes {}, local deletes {}, remote pages {}, remote items {}, local items {})",
        stats.downloaded_files,
        stats.uploaded_files,
        stats.deleted_remote,
        stats.deleted_local,
        stats.remote_pages,
        stats.remote_items_received,
        stats.local_items_seen
    );
    let _ = activity_log::append_event(
        profile_id,
        &profile.email,
        "success",
        &format!("{} [cycle:{}] {}", account_prefix, cycle_id, summary),
    );
    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "idle",
        "Idle - waiting for next sync cycle",
    );
    Ok(stats)
}

fn new_cycle_id() -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let pid = std::process::id();
    format!("{}-{}", nanos, pid)
}

fn remaining_until_next_cycle(profile_id: &str, interval: Duration) -> Option<Duration> {
    let state = load_sync_state(profile_id).ok()?;
    let last_cycle_at = state.last_cycle_at?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(&last_cycle_at).ok()?;
    let elapsed = chrono::Utc::now().signed_duration_since(timestamp.with_timezone(&chrono::Utc));
    if elapsed.num_milliseconds() <= 0 {
        return Some(interval);
    }
    let elapsed_duration = Duration::from_millis(elapsed.num_milliseconds() as u64);
    if elapsed_duration >= interval {
        return None;
    }
    Some(interval - elapsed_duration)
}

fn load_syncable_profile(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<AccountProfile, String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profiles = load_profiles()?;
    let profile = profiles
        .into_iter()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    if profile.agent_state != "syncing" {
        return Err("Account is not in syncing state".to_string());
    }
    if !profile.auth_configured {
        return Err("Account is not authenticated".to_string());
    }
    Ok(profile)
}

fn update_profile_last_sync(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<(), String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    profile.last_sync_at = Some(chrono::Local::now().to_rfc3339());
    save_profiles(&profiles)
}

async fn fetch_delta_changes(
    graph: &mut GraphContext,
    initial_delta_link: Option<String>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<Vec<DeltaItem>, String> {
    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "scanning_remote",
        "Fetching remote file list",
    );
    let mut all_items: Vec<DeltaItem> = Vec::new();
    let mut current_url =
        initial_delta_link.unwrap_or_else(|| format!("{GRAPH_ROOT}/me/drive/root/delta"));

    loop {
        ensure_not_cancelled(cancel_flag)?;
        log::info!(
            "{} [cycle:{}] DELTA_PAGE_REQUEST url={}",
            graph.account_prefix,
            graph.cycle_id,
            current_url
        );
        let response_text = graph_get_text(graph, &current_url, cancel_flag).await?;
        let response: DeltaResponse = serde_json::from_str(&response_text)
            .map_err(|error| format!("Failed to decode delta response: {error}"))?;

        stats.remote_pages += 1;
        stats.remote_items_received += response.value.len();
        log::info!(
            "{} [cycle:{}] DELTA_PAGE_RECEIVED page={} items={}",
            graph.account_prefix,
            graph.cycle_id,
            stats.remote_pages,
            response.value.len()
        );
        for item in &response.value {
            let path = resolve_delta_item_path(item).unwrap_or_else(|| "<unknown>".to_string());
            log::info!(
                "{} [cycle:{}] DELTA_ITEM id={} path={} is_dir={} deleted={}",
                graph.account_prefix,
                graph.cycle_id,
                item.id,
                path,
                item.folder.is_some(),
                item.deleted.is_some()
            );
        }
        all_items.extend(response.value.into_iter());

        if let Some(next_link) = response.next_link {
            current_url = next_link;
            continue;
        }

        if let Some(delta_link) = response.delta_link {
            sync_state.delta_link = Some(delta_link);
        }
        break;
    }

    Ok(all_items)
}

