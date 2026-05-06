use crate::app::account_profiles::load_profiles;
use crate::app::account_profiles::AccountProfile;
use crate::app::state::AppState;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatusSnapshot {
    app_version: String,
    platform: String,
    sync_engine_ready: bool,
    auth_configured: bool,
    active_account: Option<String>,
    last_sync_at: Option<String>,
    health: String,
    accounts: Vec<AccountProfile>,
}

#[tauri::command]
pub fn get_status_snapshot(state: tauri::State<'_, AppState>) -> Result<AppStatusSnapshot, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let accounts = load_profiles()?;

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let platform = std::env::consts::OS.to_string();
    let auth_configured = accounts.iter().any(|account| account.auth_configured);
    let active_account = accounts.first().map(|account| account.display_name.clone());
    let last_sync_at = accounts
        .iter()
        .find_map(|account| account.last_sync_at.clone());
    let health = if accounts
        .iter()
        .any(|account| account.agent_state == "error")
    {
        "error".to_string()
    } else if accounts.is_empty() {
        "degraded".to_string()
    } else {
        "ok".to_string()
    };

    Ok(AppStatusSnapshot {
        app_version,
        platform,
        sync_engine_ready: !accounts.is_empty(),
        auth_configured,
        active_account,
        last_sync_at,
        health,
        accounts,
    })
}
