use crate::app::state::AppState;
use crate::app::sync_runtime::{snapshot, SyncRuntimeSnapshot};

#[tauri::command]
pub fn get_sync_runtime_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<SyncRuntimeSnapshot, String> {
    let runtime = state
        .sync_runtime
        .lock()
        .map_err(|_| "Sync runtime lock is poisoned".to_string())?;
    Ok(snapshot(&runtime))
}
