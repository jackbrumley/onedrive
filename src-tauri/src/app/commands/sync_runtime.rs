use crate::app::state::AppState;
use crate::app::sync_engine::hydrate_runtime_status_from_db;
use crate::app::sync_runtime::{snapshot, SyncRuntimeSnapshot};

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

    Ok(runtime_snapshot)
}
