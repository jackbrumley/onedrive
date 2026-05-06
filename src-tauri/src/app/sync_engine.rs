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
        start_sync_worker(state, profile_id)?;
    } else {
        let _ = set_cancel_flag(state, profile_id, true)?;
        stop_sync_worker(state, profile_id)?;
        if let Ok(mut runtime_map) = state.sync_runtime.lock() {
            sync_runtime::clear_in_progress(&mut runtime_map, profile_id);
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

async fn apply_remote_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    changes: &[DeltaItem],
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_remote",
        "Applying remote changes",
    );
    let mut pending_downloads: Vec<(String, String, PathBuf, RemoteKnownItem)> = Vec::new();

    for item in changes {
        ensure_not_cancelled(cancel_flag)?;
        if item.deleted.is_some() {
            if let Some(existing) = sync_state.remote_by_id.get(&item.id).cloned() {
                log::info!(
                    "{} [cycle:{}] REMOTE_DELETE_ITEM id={} path={} is_dir={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    item.id,
                    existing.path,
                    existing.is_dir
                );
                let local_abs = sync_root.join(path_to_local(&existing.path));
                let local_current = read_local_entry(&local_abs)?;
                let previous_local = sync_state.local_snapshot.get(&existing.path);
                let local_changed = local_current
                    .as_ref()
                    .map(|entry| has_local_changed(entry, previous_local))
                    .unwrap_or(false);

                if local_changed && !existing.is_dir {
                    log::info!(
                        "{} [cycle:{}] REMOTE_DELETE_LOCAL_CHANGED_UPLOAD path={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        existing.path
                    );
                    let uploaded =
                        upload_file_by_path(graph, sync_root, &existing.path, cancel_flag).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &existing.path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                sync_state.remote_by_id.remove(&item.id);
                sync_state.remote_path_to_id.remove(&existing.path);
                sync_state.local_snapshot.remove(&existing.path);
                remove_local_path(sync_root, &existing.path)?;
                log::info!(
                    "{} [cycle:{}] LOCAL_DELETE_OK path={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    existing.path
                );
                stats.deleted_local += 1;
            }
            continue;
        }

        let Some(path) = resolve_delta_item_path(item) else {
            log::warn!(
                "{} [cycle:{}] DELTA_ITEM_SKIPPED id={} reason=missing_path",
                graph.account_prefix,
                graph.cycle_id,
                item.id
            );
            continue;
        };

        let remote_entry = RemoteKnownItem {
            id: item.id.clone(),
            path: path.clone(),
            is_dir: item.folder.is_some(),
            size: item.size.unwrap_or(0),
            modified_ts: parse_rfc3339_seconds(item.last_modified_date_time.as_deref()),
        };

        let local_abs = sync_root.join(path_to_local(&path));
        if remote_entry.is_dir {
            log::info!(
                "{} [cycle:{}] LOCAL_DIR_ENSURE path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            std::fs::create_dir_all(&local_abs).map_err(|error| {
                format!(
                    "Failed creating local directory '{}': {}",
                    local_abs.display(),
                    error
                )
            })?;
        } else {
            let local_current = read_local_entry(&local_abs)?;
            let previous_local = sync_state.local_snapshot.get(&path);
            let local_changed = local_current
                .as_ref()
                .map(|entry| has_local_changed(entry, previous_local))
                .unwrap_or(false);

            if local_changed {
                let local_entry = local_current.expect("local_changed implies local entry exists");
                if local_entry.modified_ts > remote_entry.modified_ts {
                    log::info!(
                        "{} [cycle:{}] REMOTE_OLDER_UPLOAD_LOCAL path={} local_ts={} remote_ts={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        path,
                        local_entry.modified_ts,
                        remote_entry.modified_ts
                    );
                    let uploaded =
                        upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                if let Some(backup_path) = create_safe_backup(&local_abs)? {
                    log::info!(
                        "{} [cycle:{}] SAFE_BACKUP_CREATED source={} backup={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        local_abs.display(),
                        backup_path.display()
                    );
                }
            }

            pending_downloads.push((item.id.clone(), path, local_abs, remote_entry.clone()));
            continue;
        }

        upsert_remote_known_item(sync_state, remote_entry);
    }

    if !pending_downloads.is_empty() {
        let download_concurrency = resolve_download_concurrency();
        log::info!(
            "{} [cycle:{}] REMOTE_DOWNLOAD_BATCH_START queued={} concurrency={}",
            graph.account_prefix,
            graph.cycle_id,
            pending_downloads.len(),
            download_concurrency
        );

        let mut download_tasks = stream::iter(pending_downloads.into_iter().map(|download| {
            let cancel_state = Arc::clone(cancel_flag);
            let graph_context = graph.clone();
            async move {
                let (item_id, path, local_abs, remote_entry) = download;
                match download_remote_item_content(
                    &graph_context,
                    &item_id,
                    &path,
                    &local_abs,
                    &cancel_state,
                )
                .await
                {
                    Ok(()) => Ok((item_id, path, remote_entry)),
                    Err(error) => Err(format!(
                        "Remote download failed item_id={} path={}: {}",
                        item_id, path, error
                    )),
                }
            }
        }))
        .buffer_unordered(download_concurrency);

        let mut completed_count: usize = 0;
        while let Some(task_result) = download_tasks.next().await {
            let (_, _, remote_entry) = task_result?;
            upsert_remote_known_item(sync_state, remote_entry);
            stats.downloaded_files += 1;
            completed_count += 1;
        }

        log::info!(
            "{} [cycle:{}] REMOTE_DOWNLOAD_BATCH_COMPLETE completed={}",
            graph.account_prefix,
            graph.cycle_id,
            completed_count
        );
    }

    Ok(())
}

async fn apply_local_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    current_local_snapshot: &HashMap<String, LocalSnapshotEntry>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_local",
        "Applying local changes",
    );
    let mut local_paths: Vec<String> = current_local_snapshot.keys().cloned().collect();
    local_paths.sort_by_key(|path| path.matches('/').count());
    for path in local_paths {
        ensure_not_cancelled(cancel_flag)?;
        let Some(local_entry) = current_local_snapshot.get(&path) else {
            continue;
        };
        let previous_local = sync_state.local_snapshot.get(&path);
        let local_changed = has_local_changed(local_entry, previous_local);
        let remote_id = sync_state.remote_path_to_id.get(&path).cloned();

        if local_entry.is_dir {
            if remote_id.is_none() {
                log::info!(
                    "{} [cycle:{}] REMOTE_DIR_CREATE_START path={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    path
                );
                let created = create_remote_folder(graph, &path, cancel_flag).await?;
                let known = remote_known_item_from_drive_item(created, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.created_remote_folders += 1;
            }
            continue;
        }

        if !local_changed {
            continue;
        }

        log::info!(
            "{} [cycle:{}] LOCAL_CHANGE path={} is_dir={} size={} modified_ts={}",
            graph.account_prefix,
            graph.cycle_id,
            path,
            local_entry.is_dir,
            local_entry.size,
            local_entry.modified_ts
        );

        if let Some(existing_id) = remote_id {
            let remote_modified = sync_state
                .remote_by_id
                .get(&existing_id)
                .map(|item| item.modified_ts)
                .unwrap_or(0);
            if local_entry.modified_ts >= remote_modified {
                log::info!(
                    "{} [cycle:{}] LOCAL_UPLOAD_EXISTING path={} remote_id={} local_ts={} remote_ts={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    path,
                    existing_id,
                    local_entry.modified_ts,
                    remote_modified
                );
                let uploaded = upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
                let known = remote_known_item_from_drive_item(uploaded, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.uploaded_files += 1;
            }
        } else {
            log::info!(
                "{} [cycle:{}] LOCAL_UPLOAD_NEW path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            let uploaded = upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
            let known = remote_known_item_from_drive_item(uploaded, &path)?;
            upsert_remote_known_item(sync_state, known);
            stats.uploaded_files += 1;
        }
    }

    let deleted_paths: Vec<String> = sync_state
        .local_snapshot
        .keys()
        .filter(|path| !current_local_snapshot.contains_key(*path))
        .cloned()
        .collect();

    let mut deleted_paths = deleted_paths;
    deleted_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));

    for deleted_path in deleted_paths {
        ensure_not_cancelled(cancel_flag)?;
        if let Some(remote_id) = sync_state.remote_path_to_id.get(&deleted_path).cloned() {
            log::info!(
                "{} [cycle:{}] REMOTE_DELETE_START path={} remote_id={}",
                graph.account_prefix,
                graph.cycle_id,
                deleted_path,
                remote_id
            );
            delete_remote_item(graph, &remote_id, cancel_flag).await?;
            sync_state.remote_path_to_id.remove(&deleted_path);
            sync_state.remote_by_id.remove(&remote_id);
            log::info!(
                "{} [cycle:{}] REMOTE_DELETE_OK path={} remote_id={}",
                graph.account_prefix,
                graph.cycle_id,
                deleted_path,
                remote_id
            );
            stats.deleted_remote += 1;
        }
    }

    Ok(())
}

fn upsert_remote_known_item(sync_state: &mut PersistedSyncState, item: RemoteKnownItem) {
    sync_state
        .remote_path_to_id
        .insert(item.path.clone(), item.id.clone());
    sync_state.remote_by_id.insert(item.id.clone(), item);
}

fn has_local_changed(current: &LocalSnapshotEntry, previous: Option<&LocalSnapshotEntry>) -> bool {
    match previous {
        Some(entry) => entry != current,
        None => true,
    }
}

fn remove_local_path(sync_root: &Path, relative_path: &str) -> Result<(), String> {
    let full_path = sync_root.join(path_to_local(relative_path));
    if !full_path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&full_path).map_err(|error| error.to_string())?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(&full_path).map_err(|error| {
            format!(
                "Failed removing directory '{}': {}",
                full_path.display(),
                error
            )
        })
    } else {
        std::fs::remove_file(&full_path)
            .map_err(|error| format!("Failed removing file '{}': {}", full_path.display(), error))
    }
}

fn create_safe_backup(local_path: &Path) -> Result<Option<PathBuf>, String> {
    if !local_path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(local_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Ok(None);
    }

    let parent = local_path
        .parent()
        .ok_or_else(|| "Local backup path has no parent".to_string())?;
    let file_name = local_path
        .file_name()
        .ok_or_else(|| "Local backup path has no filename".to_string())?
        .to_string_lossy()
        .to_string();

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut index = 1_u32;
    loop {
        let backup_name = format!("{}-safeBackup-{}-{:04}", file_name, stamp, index);
        let backup_path = parent.join(backup_name);
        if !backup_path.exists() {
            std::fs::copy(local_path, &backup_path).map_err(|error| {
                format!(
                    "Failed creating safe backup '{}' from '{}': {}",
                    backup_path.display(),
                    local_path.display(),
                    error
                )
            })?;
            return Ok(Some(backup_path));
        }
        index += 1;
    }
}

fn resolve_delta_item_path(item: &DeltaItem) -> Option<String> {
    let name = item.name.as_ref()?.trim();
    if name.is_empty() {
        return None;
    }
    let base = item
        .parent_reference
        .as_ref()
        .and_then(|reference| reference.path.as_deref())
        .map(extract_root_relative)
        .unwrap_or_default();
    let combined = if base.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base, name)
    };
    Some(normalize_relative_path(&combined))
}

fn extract_root_relative(parent_path: &str) -> String {
    let mut value = parent_path.trim().to_string();
    if let Some(rest) = value.strip_prefix("/drive/root:") {
        value = rest.to_string();
    }
    value.trim_start_matches('/').to_string()
}

fn normalize_relative_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_to_local(relative_path: &str) -> PathBuf {
    let mut output = PathBuf::new();
    for segment in relative_path.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

fn parse_rfc3339_seconds(value: Option<&str>) -> i64 {
    value
        .and_then(|input| chrono::DateTime::parse_from_rfc3339(input).ok())
        .map(|timestamp| timestamp.timestamp())
        .unwrap_or(0)
}

async fn graph_get_text(
    graph: &mut GraphContext,
    url: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(url)
                .bearer_auth(&graph.access_token)
                .send() => {
                value.map_err(|error| format!("Graph GET failed: {error}"))?
            }
        };
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading Graph response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_GET_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Graph GET {} failed with status {}: {}",
                url, status, snippet
            ));
        }
        return Ok(text);
    }
}

async fn graph_delete(graph: &mut GraphContext, url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        let response = client
            .delete(url)
            .bearer_auth(&graph.access_token)
            .send()
            .await
            .map_err(|error| format!("Graph DELETE failed: {error}"))?;
        let status = response.status();
        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DELETE_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }

        let text = response.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(400).collect();
        return Err(format!(
            "Graph DELETE {} failed with status {}: {}",
            url, status, snippet
        ));
    }
}

async fn download_remote_item_content(
    graph: &GraphContext,
    item_id: &str,
    relative_path: &str,
    local_path: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    ensure_not_cancelled(cancel_flag)?;
    log::info!(
        "{} [cycle:{}] DOWNLOAD_START item_id={} path={} local_path={}",
        graph.account_prefix,
        graph.cycle_id,
        item_id,
        relative_path,
        local_path.display()
    );
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}/content", item_id);
    let client = reqwest::Client::new();
    let mut access_token = graph.access_token.clone();
    let mut token_refreshed = false;
    let mut attempt: u32 = 0;
    let transfer_id = runtime_start_transfer(
        &graph.sync_runtime,
        &graph.profile_id,
        "download",
        relative_path,
        None,
    );
    loop {
        ensure_not_cancelled(cancel_flag)?;
        attempt += 1;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        SYNC_CANCELLED_ERROR,
                    );
                }
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(&url)
                .bearer_auth(&access_token)
                .send() => {
                value.map_err(|error| format!("Download request failed: {error}"))
            }
        };
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let delay = exponential_backoff_delay(attempt);
                    log::warn!(
                        "{} [cycle:{}] DOWNLOAD_RETRY attempt={} path={} reason={} delay_ms={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        attempt,
                        relative_path,
                        error,
                        delay.as_millis()
                    );
                    sleep_with_cancellation(cancel_flag, delay).await?;
                    continue;
                }
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        &error,
                    );
                }
                return Err(error);
            }
        };
        let status = response.status();
        if status.as_u16() == 401 && !token_refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DOWNLOAD_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let refreshed = refresh_access_token(&graph.profile_id).await?;
            access_token = refreshed.access_token;
            token_refreshed = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            if attempt < MAX_DOWNLOAD_RETRIES {
                let delay = parse_retry_after_delay(response.headers())
                    .unwrap_or_else(|| exponential_backoff_delay(attempt));
                log::warn!(
                    "{} [cycle:{}] DOWNLOAD_RETRY_HTTP attempt={} status={} path={} delay_ms={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    attempt,
                    status,
                    relative_path,
                    delay.as_millis()
                );
                sleep_with_cancellation(cancel_flag, delay).await?;
                continue;
            }
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(400).collect();
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Download failed with status {}", status),
                );
            }
            return Err(format!(
                "Download failed for item {} with status {}: {}",
                item_id, status, snippet
            ));
        }

        let total = response.content_length();
        if let Some(active_transfer_id) = &transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                0,
                total,
            );
        }

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed creating local parent '{}': {}",
                    parent.display(),
                    error
                )
            })?;
        }

        let temp_path = local_path.with_extension("somedrive-part");
        let mut output_file = std::fs::File::create(&temp_path).map_err(|error| {
            format!(
                "Failed creating temporary download file '{}': {}",
                temp_path.display(),
                error
            )
        })?;

        let mut stream = response.bytes_stream();
        let mut downloaded_bytes: u64 = 0;
        while let Some(chunk_result) = stream.next().await {
            if cancel_flag.load(Ordering::Relaxed) {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        SYNC_CANCELLED_ERROR,
                    );
                }
                let _ = std::fs::remove_file(&temp_path);
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            let chunk = match chunk_result {
                Ok(value) => value,
                Err(error) => {
                    if let Some(active_transfer_id) = &transfer_id {
                        runtime_finish_transfer_error(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            active_transfer_id,
                            &format!("Failed reading download stream: {}", error),
                        );
                    }
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(format!("Failed reading download bytes stream: {error}"));
                }
            };

            if let Err(error) = output_file.write_all(&chunk) {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        &format!("Failed writing download chunk: {}", error),
                    );
                }
                let _ = std::fs::remove_file(&temp_path);
                return Err(format!(
                    "Failed writing temporary file '{}': {}",
                    temp_path.display(),
                    error
                ));
            }

            downloaded_bytes += chunk.len() as u64;
            if let Some(active_transfer_id) = &transfer_id {
                runtime_update_transfer_progress(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    downloaded_bytes,
                    total,
                );
            }
        }

        if let Err(error) = output_file.flush() {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Failed flushing temporary file: {}", error),
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Failed flushing temporary file '{}': {}",
                temp_path.display(),
                error
            ));
        }

        if cancel_flag.load(Ordering::Relaxed) {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    SYNC_CANCELLED_ERROR,
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(SYNC_CANCELLED_ERROR.to_string());
        }

        if let Err(error) = std::fs::rename(&temp_path, local_path) {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Failed finalizing local file: {}", error),
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Failed moving '{}' to '{}': {}",
                temp_path.display(),
                local_path.display(),
                error
            ));
        }

        if let Some(active_transfer_id) = &transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                downloaded_bytes,
                Some(downloaded_bytes),
            );
            runtime_finish_transfer_success(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
            );
        }

        log::info!(
            "{} [cycle:{}] DOWNLOAD_OK item_id={} path={} bytes={}",
            graph.account_prefix,
            graph.cycle_id,
            item_id,
            relative_path,
            downloaded_bytes
        );
        break;
    }

    Ok(())
}

async fn upload_file_by_path(
    graph: &mut GraphContext,
    sync_root: &Path,
    relative_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    ensure_not_cancelled(cancel_flag)?;
    let local_path = sync_root.join(path_to_local(relative_path));
    let content = std::fs::read(&local_path).map_err(|error| {
        format!(
            "Failed reading local file '{}': {}",
            local_path.display(),
            error
        )
    })?;

    let encoded_path = encode_graph_path(relative_path);
    let url = format!("{GRAPH_ROOT}/me/drive/root:/{}:/content", encoded_path);
    let content_len = content.len() as u64;
    let transfer_id = runtime_start_transfer(
        &graph.sync_runtime,
        &graph.profile_id,
        "upload",
        relative_path,
        Some(content_len),
    );
    log::info!(
        "{} [cycle:{}] UPLOAD_START path={} local_path={} bytes={}",
        graph.account_prefix,
        graph.cycle_id,
        relative_path,
        local_path.display(),
        content.len()
    );
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        SYNC_CANCELLED_ERROR,
                    );
                }
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .put(&url)
                .bearer_auth(&graph.access_token)
                .body(content.clone())
                .send() => {
                value.map_err(|error| {
                    if let Some(active_transfer_id) = &transfer_id {
                        runtime_finish_transfer_error(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            active_transfer_id,
                            &format!("Upload request failed: {}", error),
                        );
                    }
                    format!("Failed uploading file '{}': {}", relative_path, error)
                })?
            }
        };

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading upload response: {error}"))?;

        ensure_not_cancelled(cancel_flag)?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_UPLOAD_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Upload failed with status {}", status),
                );
            }
            return Err(format!(
                "Upload failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
        let parsed = serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding upload response JSON: {error}"))?;
        if let Some(active_transfer_id) = &transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                content_len,
                Some(content_len),
            );
            runtime_finish_transfer_success(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
            );
        }
        log::info!(
            "{} [cycle:{}] UPLOAD_OK path={} remote_id={} size={}",
            graph.account_prefix,
            graph.cycle_id,
            relative_path,
            parsed.id,
            parsed.size.unwrap_or(0)
        );
        return Ok(parsed);
    }
}

async fn create_remote_folder(
    graph: &mut GraphContext,
    relative_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    ensure_not_cancelled(cancel_flag)?;
    let (parent, name) = split_parent_and_name(relative_path)?;
    let endpoint = if parent.is_empty() {
        format!("{GRAPH_ROOT}/me/drive/root/children")
    } else {
        format!(
            "{GRAPH_ROOT}/me/drive/root:/{}:/children",
            encode_graph_path(parent)
        )
    };
    let payload = serde_json::json!({
        "name": name,
        "folder": {},
        "@microsoft.graph.conflictBehavior": "replace"
    });
    log::info!(
        "{} [cycle:{}] REMOTE_DIR_CREATE_REQUEST path={} parent={}",
        graph.account_prefix,
        graph.cycle_id,
        relative_path,
        parent
    );

    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .post(&endpoint)
                .bearer_auth(&graph.access_token)
                .json(&payload)
                .send() => {
                value.map_err(|error| {
                    format!(
                        "Failed creating remote folder '{}': {}",
                        relative_path, error
                    )
                })?
            }
        };

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading create folder response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DIR_CREATE_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Create folder failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
        let parsed = serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding create-folder response JSON: {error}"))?;
        log::info!(
            "{} [cycle:{}] REMOTE_DIR_CREATE_OK path={} remote_id={}",
            graph.account_prefix,
            graph.cycle_id,
            relative_path,
            parsed.id
        );
        return Ok(parsed);
    }
}

async fn delete_remote_item(
    graph: &mut GraphContext,
    item_id: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    ensure_not_cancelled(cancel_flag)?;
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}", item_id);
    log::info!(
        "{} [cycle:{}] REMOTE_DELETE_REQUEST item_id={}",
        graph.account_prefix,
        graph.cycle_id,
        item_id
    );
    graph_delete(graph, &url).await?;
    log::info!(
        "{} [cycle:{}] REMOTE_DELETE_RESULT item_id={} status=ok",
        graph.account_prefix,
        graph.cycle_id,
        item_id
    );
    Ok(())
}

fn split_parent_and_name(relative_path: &str) -> Result<(&str, &str), String> {
    let trimmed = relative_path.trim_matches('/');
    if trimmed.is_empty() {
        return Err("Remote folder path is empty".to_string());
    }
    if let Some((parent, name)) = trimmed.rsplit_once('/') {
        return Ok((parent, name));
    }
    Ok(("", trimmed))
}

fn encode_graph_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        parts.push(percent_encode(segment));
    }
    parts.join("/")
}

fn percent_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        let unreserved = matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if unreserved {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push_str(&format!("{:02X}", byte));
        }
    }
    output
}

fn remote_known_item_from_drive_item(
    item: DriveItemResponse,
    fallback_path: &str,
) -> Result<RemoteKnownItem, String> {
    let path = if let (Some(name), Some(parent_ref)) =
        (item.name.clone(), item.parent_reference.clone())
    {
        let parent = parent_ref
            .path
            .as_deref()
            .map(extract_root_relative)
            .unwrap_or_default();
        normalize_relative_path(&if parent.is_empty() {
            name
        } else {
            format!("{}/{}", parent, name)
        })
    } else {
        fallback_path.to_string()
    };
    if path.trim().is_empty() {
        return Err("Remote item path cannot be empty".to_string());
    }

    Ok(RemoteKnownItem {
        id: item.id,
        path,
        is_dir: item.folder.is_some(),
        size: item.size.unwrap_or(0),
        modified_ts: parse_rfc3339_seconds(item.last_modified_date_time.as_deref()),
    })
}

fn collect_local_snapshot(sync_root: &Path) -> Result<HashMap<String, LocalSnapshotEntry>, String> {
    let mut snapshot = HashMap::new();
    if !sync_root.exists() {
        return Ok(snapshot);
    }

    let mut stack: Vec<PathBuf> = vec![sync_root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|error| {
            format!(
                "Failed reading directory '{}': {}",
                current.display(),
                error
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = entry.metadata().map_err(|error| {
                format!("Failed reading metadata '{}': {}", path.display(), error)
            })?;

            if !metadata.is_file() && !metadata.is_dir() {
                continue;
            }

            let relative = path
                .strip_prefix(sync_root)
                .map_err(|error| {
                    format!(
                        "Failed computing relative path '{}': {}",
                        path.display(),
                        error
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            let normalized = normalize_relative_path(&relative);
            if normalized.is_empty() {
                continue;
            }

            let modified_ts = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|value| value.as_secs() as i64)
                .unwrap_or(0);

            snapshot.insert(
                normalized,
                LocalSnapshotEntry {
                    is_dir: metadata.is_dir(),
                    size: if metadata.is_file() {
                        metadata.len()
                    } else {
                        0
                    },
                    modified_ts,
                },
            );

            if metadata.is_dir() {
                stack.push(path);
            }
        }
    }

    Ok(snapshot)
}

fn read_local_entry(path: &Path) -> Result<Option<LocalSnapshotEntry>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let modified_ts = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0);
    Ok(Some(LocalSnapshotEntry {
        is_dir: metadata.is_dir(),
        size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        modified_ts,
    }))
}

fn sync_state_path(profile_id: &str) -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir
        .join("somedrive")
        .join("accounts")
        .join(profile_id)
        .join("sync_state.json"))
}

fn load_sync_state(profile_id: &str) -> Result<PersistedSyncState, String> {
    let path = sync_state_path(profile_id)?;
    if !path.exists() {
        return Ok(PersistedSyncState::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("Failed reading sync state '{}': {}", path.display(), error))?;
    serde_json::from_str::<PersistedSyncState>(&text)
        .map_err(|error| format!("Failed decoding sync state JSON: {error}"))
}

fn save_sync_state(profile_id: &str, state: &PersistedSyncState) -> Result<(), String> {
    let path = sync_state_path(profile_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed creating sync state directory '{}': {}",
                parent.display(),
                error
            )
        })?;
    }
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Failed encoding sync state JSON: {error}"))?;
    std::fs::write(&path, text)
        .map_err(|error| format!("Failed writing sync state '{}': {}", path.display(), error))
}
