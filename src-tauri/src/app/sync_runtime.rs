use chrono::Local;
use serde::Serialize;
use std::collections::HashMap;

const RECENT_COMPLETED_LIMIT: usize = 40;
const RECENT_FAILED_LIMIT: usize = 40;

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
    pub in_progress: Vec<SyncRuntimeTransfer>,
    pub recent_completed: Vec<SyncRuntimeRecentItem>,
    pub recent_failed: Vec<SyncRuntimeRecentItem>,
    pub updated_at: String,
}

impl SyncRuntimeAccountStatus {
    fn new(profile_id: &str) -> Self {
        let now = now_rfc3339();
        Self {
            profile_id: profile_id.to_string(),
            phase: "idle".to_string(),
            phase_message: "Idle".to_string(),
            in_progress: Vec::new(),
            recent_completed: Vec::new(),
            recent_failed: Vec::new(),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRuntimeSnapshot {
    pub generated_at: String,
    pub accounts: Vec<SyncRuntimeAccountStatus>,
}

pub type SyncRuntimeMap = HashMap<String, SyncRuntimeAccountStatus>;

pub fn snapshot(runtime_map: &SyncRuntimeMap) -> SyncRuntimeSnapshot {
    let mut accounts: Vec<SyncRuntimeAccountStatus> = runtime_map.values().cloned().collect();
    accounts.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    SyncRuntimeSnapshot {
        generated_at: now_rfc3339(),
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
    status.updated_at = now_rfc3339();
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
}

pub fn finish_transfer_success(
    runtime_map: &mut SyncRuntimeMap,
    profile_id: &str,
    transfer_id: &str,
) {
    if let Some(item) = remove_transfer(runtime_map, profile_id, transfer_id) {
        let status = ensure_account_status(runtime_map, profile_id);
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
        status.updated_at = now_rfc3339();
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
        status.updated_at = now_rfc3339();
    }
}

pub fn clear_in_progress(runtime_map: &mut SyncRuntimeMap, profile_id: &str) {
    let status = ensure_account_status(runtime_map, profile_id);
    status.in_progress.clear();
    status.updated_at = now_rfc3339();
}

pub fn remove_account(runtime_map: &mut SyncRuntimeMap, profile_id: &str) {
    runtime_map.remove(profile_id);
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

fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}

fn new_runtime_id(prefix: &str) -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("{}-{}", prefix, nanos)
}
