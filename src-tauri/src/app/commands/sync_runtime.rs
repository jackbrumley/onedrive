use crate::app::state::AppState;
use crate::app::sync_engine::hydrate_runtime_status_from_db;
use crate::app::sync_runtime::{emit_full_sync_status_snapshot, snapshot, SyncRuntimeSnapshot};

fn runtime_revision_from_updated_at(snapshot: &SyncRuntimeSnapshot) -> u64 {
    snapshot
        .accounts
        .iter()
        .filter_map(|account| chrono::DateTime::parse_from_rfc3339(&account.updated_at).ok())
        .map(|value| value.timestamp_millis().max(0) as u64)
        .max()
        .unwrap_or(0)
}

#[tauri::command]
pub fn get_sync_runtime_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<SyncRuntimeSnapshot, String> {
    let runtime = state
        .sync_runtime
        .lock()
        .map_err(|_| "Sync runtime lock is poisoned".to_string())?;
    let mut runtime_snapshot = snapshot(&runtime);
    drop(runtime);

    for account in &mut runtime_snapshot.accounts {
        if let Err(error) = hydrate_runtime_status_from_db(account) {
            log::warn!(
                "{} SYNC_ACTIVITY_DB_HYDRATE_FAILED error={}",
                crate::app::log_context::account_prefix(&account.profile_id),
                error
            );
        }
    }

    runtime_snapshot.revision = runtime_snapshot
        .revision
        .max(runtime_revision_from_updated_at(&runtime_snapshot));

    Ok(runtime_snapshot)
}

#[tauri::command]
pub fn request_sync_status_snapshot(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let runtime = state
        .sync_runtime
        .lock()
        .map_err(|_| "Sync runtime lock is poisoned".to_string())?;
    emit_full_sync_status_snapshot(&runtime);
    Ok(())
}
