use chrono::Local;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

const RECENT_COMPLETED_LIMIT: usize = 120;
const RECENT_FAILED_LIMIT: usize = 120;
static SYNC_RUNTIME_REVISION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRuntimeTransfer {
    pub id: String,
    pub direction: String,
    pub path: String,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRuntimeRecentItem {
    pub id: String,
    pub direction: String,
    pub path: String,
    pub bytes_total: Option<u64>,
    pub finished_at: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRuntimeAccountStatus {
    pub profile_id: String,
    pub phase: String,
    pub phase_message: String,
    pub issue_code: Option<String>,
    pub issue_message: Option<String>,
    pub issue_actions: Vec<String>,
    pub issue_path: Option<String>,
    pub issue_secondary_path: Option<String>,
    pub in_progress: Vec<SyncRuntimeTransfer>,
    pub recent_completed: Vec<SyncRuntimeRecentItem>,
    pub recent_failed: Vec<SyncRuntimeRecentItem>,
    pub remote_discovered_count: usize,
    pub remote_download_queue_count: usize,
    pub remote_downloaded_count: usize,
    pub remote_discovered_total: usize,
    pub remote_download_planned_total: usize,
    pub remote_download_completed_total: usize,
    pub remote_download_failed_total: usize,
    pub remote_download_in_flight: usize,
    pub remote_download_retry_waiting: usize,
    pub upload_planned_total: usize,
    pub upload_completed_total: usize,
    pub upload_failed_total: usize,
    pub upload_in_flight: usize,
    pub remote_scan_complete: bool,
    pub updated_at: String,
    #[serde(skip_serializing)]
    remote_session_discovered_ids: HashSet<String>,
    #[serde(skip_serializing)]
    remote_session_planned_ids: HashSet<String>,
    #[serde(skip_serializing)]
    remote_session_completed_ids: HashSet<String>,
    #[serde(skip_serializing)]
    remote_session_failed_ids: HashSet<String>,
}

impl SyncRuntimeAccountStatus {
    fn new(profile_id: &str) -> Self {
        let now = now_rfc3339();
        Self {
            profile_id: profile_id.to_string(),
            phase: "idle".to_string(),
            phase_message: "Idle".to_string(),
            issue_code: None,
            issue_message: None,
            issue_actions: Vec::new(),
            issue_path: None,
            issue_secondary_path: None,
            in_progress: Vec::new(),
            recent_completed: Vec::new(),
            recent_failed: Vec::new(),
            remote_discovered_count: 0,
            remote_download_queue_count: 0,
            remote_downloaded_count: 0,
            remote_discovered_total: 0,
            remote_download_planned_total: 0,
            remote_download_completed_total: 0,
            remote_download_failed_total: 0,
            remote_download_in_flight: 0,
            remote_download_retry_waiting: 0,
            upload_planned_total: 0,
            upload_completed_total: 0,
            upload_failed_total: 0,
            upload_in_flight: 0,
            remote_scan_complete: false,
            updated_at: now,
            remote_session_discovered_ids: HashSet::new(),
            remote_session_planned_ids: HashSet::new(),
            remote_session_completed_ids: HashSet::new(),
            remote_session_failed_ids: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRuntimeSnapshot {
    pub generated_at: String,
    pub revision: u64,
    pub accounts: Vec<SyncRuntimeAccountStatus>,
}

pub type SyncRuntimeMap = HashMap<String, SyncRuntimeAccountStatus>;

pub fn snapshot(runtime_map: &SyncRuntimeMap) -> SyncRuntimeSnapshot {
    let mut accounts: Vec<SyncRuntimeAccountStatus> = runtime_map.values().cloned().collect();
    accounts.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    SyncRuntimeSnapshot {
        generated_at: now_rfc3339(),
        revision: current_runtime_revision(),
        accounts,
    }
}

pub fn set_phase(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    phase: &str,
    phase_message: &str,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.phase = phase.to_string();
    status.phase_message = phase_message.to_string();
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn set_issue(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    issue_code: &str,
    issue_message: &str,
    issue_actions: &[&str],
    issue_path: Option<&str>,
    issue_secondary_path: Option<&str>,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.issue_code = Some(issue_code.to_string());
    status.issue_message = Some(issue_message.to_string());
    status.issue_actions = issue_actions
        .iter()
        .map(|action| (*action).to_string())
        .collect();
    status.issue_path = issue_path.map(|value| value.to_string());
    status.issue_secondary_path = issue_secondary_path.map(|value| value.to_string());
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn clear_issue(runtime_map: &mut SyncRuntimeMap, profile_id: &str) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.issue_code = None;
    status.issue_message = None;
    status.issue_actions.clear();
    status.issue_path = None;
    status.issue_secondary_path = None;
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn set_remote_transfer_progress(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    discovered_count: usize,
    download_queue_count: usize,
    downloaded_count: usize,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    if discovered_count == 0 && download_queue_count == 0 && downloaded_count == 0 {
        reset_remote_session_progress(status);
        reset_upload_session_progress(status);
    }
    status.remote_discovered_count = discovered_count;
    status.remote_download_queue_count = download_queue_count;
    status.remote_downloaded_count = downloaded_count;
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn set_remote_scan_complete(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    complete: bool,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.remote_scan_complete = complete;
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn record_remote_discovered(runtime_map: &mut SyncRuntimeMap, profile_id: &str, item_id: &str) {
    let status = ensure_account_status(runtime_map, profile_id);
    if status
        .remote_session_discovered_ids
        .insert(item_id.to_string())
    {
        status.remote_discovered_total += 1;
        sync_legacy_remote_progress_fields(status);
        status.updated_at = now_rfc3339();
        bump_runtime_revision();
    }
}

pub fn record_remote_download_planned(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    item_id: &str,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    if status
        .remote_session_planned_ids
        .insert(item_id.to_string())
    {
        status.remote_download_planned_total += 1;
        sync_legacy_remote_progress_fields(status);
        status.updated_at = now_rfc3339();
        bump_runtime_revision();
    }
}

pub fn record_remote_download_completed(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    item_id: &str,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    if status
        .remote_session_completed_ids
        .insert(item_id.to_string())
    {
        status.remote_download_completed_total += 1;
    }
    if status.remote_session_failed_ids.remove(item_id) {
        status.remote_download_failed_total = status.remote_download_failed_total.saturating_sub(1);
    }
    sync_legacy_remote_progress_fields(status);
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn record_remote_download_failed(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    item_id: &str,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    if status.remote_session_failed_ids.insert(item_id.to_string()) {
        status.remote_download_failed_total += 1;
        sync_legacy_remote_progress_fields(status);
        status.updated_at = now_rfc3339();
        bump_runtime_revision();
    }
}

pub fn set_remote_download_counters(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    planned_total: usize,
    completed_total: usize,
    failed_total: usize,
    in_flight: usize,
    retry_waiting: usize,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.remote_download_planned_total = planned_total;
    status.remote_download_completed_total = completed_total;
    status.remote_download_failed_total = failed_total;
    status.remote_download_in_flight = in_flight;
    status.remote_download_retry_waiting = retry_waiting;
    sync_legacy_remote_progress_fields(status);
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn set_upload_counters(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    planned_total: usize,
    completed_total: usize,
    failed_total: usize,
    in_flight: usize,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.upload_planned_total = planned_total;
    status.upload_completed_total = completed_total;
    status.upload_failed_total = failed_total;
    status.upload_in_flight = in_flight;
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn start_transfer(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    direction: &str,
    path: &str,
    bytes_total: Option<u64>,
) -> String {
    let now = now_rfc3339();
    let transfer_id = new_runtime_id("xfer");
    let status = ensure_account_status(runtime_map, profile_id);
    status.in_progress.push(SyncRuntimeTransfer {
        id: transfer_id.clone(),
        direction: direction.to_string(),
        path: path.to_string(),
        bytes_done: 0,
        bytes_total,
        started_at: now.clone(),
        updated_at: now,
    });
    if direction.eq_ignore_ascii_case("upload") {
        status.upload_planned_total += 1;
        sync_upload_in_flight(status);
    }
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
    transfer_id
}

pub fn update_transfer_progress(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    transfer_id: &str,
    bytes_done: u64,
    bytes_total: Option<u64>,
) {
    let status = ensure_account_status(runtime_map, profile_id);
    if let Some(transfer) = status
        .in_progress
        .iter_mut()
        .find(|entry| entry.id == transfer_id)
    {
        transfer.bytes_done = bytes_done;
        if bytes_total.is_some() {
            transfer.bytes_total = bytes_total;
        }
        transfer.updated_at = now_rfc3339();
    }
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn finish_transfer_success(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    transfer_id: &str,
) {
    if let Some(item) = remove_transfer(runtime_map, profile_id, transfer_id) {
        let status = ensure_account_status(runtime_map, profile_id);
        if item.direction.eq_ignore_ascii_case("upload") {
            status.upload_completed_total += 1;
        }
        status.recent_completed.insert(
            0,
            SyncRuntimeRecentItem {
                id: item.id,
                direction: item.direction,
                path: item.path,
                bytes_total: item.bytes_total,
                finished_at: now_rfc3339(),
                status: "completed".to_string(),
                error: None,
            },
        );
        if status.recent_completed.len() > RECENT_COMPLETED_LIMIT {
            status.recent_completed.truncate(RECENT_COMPLETED_LIMIT);
        }
        sync_upload_in_flight(status);
        status.updated_at = now_rfc3339();
        bump_runtime_revision();
    }
}

pub fn finish_transfer_error(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    transfer_id: &str,
    error_message: &str,
) {
    if let Some(item) = remove_transfer(runtime_map, profile_id, transfer_id) {
        let status = ensure_account_status(runtime_map, profile_id);
        if item.direction.eq_ignore_ascii_case("upload") {
            status.upload_failed_total += 1;
        }
        status.recent_failed.insert(
            0,
            SyncRuntimeRecentItem {
                id: item.id,
                direction: item.direction,
                path: item.path,
                bytes_total: item.bytes_total,
                finished_at: now_rfc3339(),
                status: "failed".to_string(),
                error: Some(error_message.to_string()),
            },
        );
        if status.recent_failed.len() > RECENT_FAILED_LIMIT {
            status.recent_failed.truncate(RECENT_FAILED_LIMIT);
        }
        sync_upload_in_flight(status);
        status.updated_at = now_rfc3339();
        bump_runtime_revision();
    }
}

pub fn clear_in_progress(runtime_map: &mut SyncRuntimeMap, profile_id: &str) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.in_progress.clear();
    sync_upload_in_flight(status);
    status.updated_at = now_rfc3339();
    bump_runtime_revision();
}

pub fn remove_account(runtime_map: &mut SyncRuntimeMap, profile_id: &str) {
    runtime_map.remove(profile_id);
    bump_runtime_revision();
}

fn remove_transfer(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    transfer_id: &str,
) -> Option<SyncRuntimeTransfer> {
    let status = ensure_account_status(runtime_map, profile_id);
    let index = status
        .in_progress
        .iter()
        .position(|entry| entry.id == transfer_id)?;
    Some(status.in_progress.remove(index))
}

fn ensure_account_status<'a>(
    runtime_map: &'a mut SyncRuntimeMap,
    profile_id: &str,
) -> &'a mut SyncRuntimeAccountStatus {
    runtime_map
        .entry(profile_id.to_string())
        .or_insert_with(|| SyncRuntimeAccountStatus::new(profile_id))
}

fn reset_remote_session_progress(status: &mut SyncRuntimeAccountStatus) {
    status.remote_discovered_total = 0;
    status.remote_download_planned_total = 0;
    status.remote_download_completed_total = 0;
    status.remote_download_failed_total = 0;
    status.remote_download_in_flight = 0;
    status.remote_download_retry_waiting = 0;
    status.remote_scan_complete = false;
    status.remote_session_discovered_ids.clear();
    status.remote_session_planned_ids.clear();
    status.remote_session_completed_ids.clear();
    status.remote_session_failed_ids.clear();
}

fn reset_upload_session_progress(status: &mut SyncRuntimeAccountStatus) {
    status.upload_planned_total = 0;
    status.upload_completed_total = 0;
    status.upload_failed_total = 0;
    status.upload_in_flight = 0;
}

fn sync_upload_in_flight(status: &mut SyncRuntimeAccountStatus) {
    status.upload_in_flight = status
        .in_progress
        .iter()
        .filter(|entry| entry.direction.eq_ignore_ascii_case("upload"))
        .count();
}

fn sync_legacy_remote_progress_fields(status: &mut SyncRuntimeAccountStatus) {
    status.remote_discovered_count = status.remote_discovered_total;
    status.remote_download_queue_count = status.remote_download_in_flight;
    status.remote_downloaded_count = status.remote_download_completed_total;
}

fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}

fn new_runtime_id(prefix: &str) -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("{}-{}", prefix, nanos)
}

fn bump_runtime_revision() {
    SYNC_RUNTIME_REVISION.fetch_add(1, Ordering::Relaxed);
}

fn current_runtime_revision() -> u64 {
    SYNC_RUNTIME_REVISION.load(Ordering::Relaxed)
}
