use crate::app::account_profiles::{load_profiles, save_profiles};
use crate::app::activity_log;
use crate::app::state::AppState;
use std::sync::Arc;

pub fn on_agent_state_changed(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
    agent_state: &str,
) -> Result<(), String> {
    if agent_state == "syncing" {
        start_sync_worker(state, profile_id)?;
    } else {
        stop_sync_worker(state, profile_id)?;
    }
    Ok(())
}

fn start_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        if stops.contains_key(profile_id) {
            return Ok(());
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        stops.insert(profile_id.to_string(), tx);

        let profile_id_owned = profile_id.to_string();
        let profiles_lock = Arc::clone(&state.profiles_lock);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        break;
                    }
                    _ = ticker.tick() => {
                        let _ = tick_sync(&profiles_lock, &profile_id_owned);
                    }
                }
            }
        });
    }

    let _ = activity_log::append_event(profile_id, profile_id, "info", "Sync agent started");
    Ok(())
}

fn stop_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let maybe_sender = {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        stops.remove(profile_id)
    };

    if let Some(sender) = maybe_sender {
        let _ = sender.send(());
        let _ = activity_log::append_event(profile_id, profile_id, "info", "Sync agent stopped");
    }

    Ok(())
}

fn tick_sync(profiles_lock: &Arc<std::sync::Mutex<()>>, profile_id: &str) -> Result<(), String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;

    if profile.agent_state != "syncing" {
        return Ok(());
    }

    profile.last_sync_at = Some(chrono::Local::now().to_rfc3339());
    let profile_name = profile.display_name.clone();
    save_profiles(&profiles)?;
    let _ = activity_log::append_event(
        profile_id,
        &profile_name,
        "success",
        "Sync heartbeat completed",
    );
    Ok(())
}
