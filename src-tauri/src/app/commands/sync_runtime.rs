use crate::app::account_profiles::load_profiles;
use crate::app::state::AppState;
use crate::app::sync_engine::hydrate_runtime_status_from_db;
use crate::app::sync_runtime::{
    emit_sync_status_snapshot_accounts, recompute_authority_fields, snapshot, SyncRuntimeSnapshot,
};

fn build_authoritative_sync_runtime_snapshot(
    state: &tauri::State<'_, AppState>,
) -> Result<SyncRuntimeSnapshot, String> {
    let runtime = state
        .sync_runtime
        .lock()
        .map_err(|_| "Sync runtime lock is poisoned".to_string())?;
    let mut runtime_snapshot = snapshot(&runtime);
    drop(runtime);

    let profile_auth_by_id: std::collections::HashMap<String, bool> = load_profiles()?
        .into_iter()
        .map(|profile| (profile.id, profile.auth_configured))
        .collect();

    for account in &mut runtime_snapshot.accounts {
        hydrate_runtime_status_from_db(account)?;
        if let Some(auth_ready) = profile_auth_by_id.get(&account.profile_id) {
            account.auth_ready = *auth_ready;
        }
        recompute_authority_fields(account);
    }

    runtime_snapshot.revision = runtime_snapshot
        .revision
        .max(runtime_revision_from_updated_at(&runtime_snapshot));

    Ok(runtime_snapshot)
}

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
    build_authoritative_sync_runtime_snapshot(&state)
}

#[tauri::command]
pub fn request_sync_status_snapshot(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let runtime_snapshot = build_authoritative_sync_runtime_snapshot(&state)?;
    emit_sync_status_snapshot_accounts(&runtime_snapshot.accounts);
    Ok(())
}
