use crate::app::account_profiles::{
    create_profile, load_profiles, remove_profile, rename_profile, save_profiles, set_agent_state,
    AccountProfile,
    CreateAccountProfileInput, RemoveAccountProfileInput, RenameAccountProfileInput,
    SetAccountAgentStateInput,
};
use crate::app::activity_log;
use crate::app::state::AppState;
use crate::app::sync_engine;
use std::fs;

#[tauri::command]
pub fn list_account_profiles(state: tauri::State<'_, AppState>) -> Result<Vec<AccountProfile>, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    load_profiles()
}

#[tauri::command]
pub fn create_account_profile(
    state: tauri::State<'_, AppState>,
    input: CreateAccountProfileInput,
) -> Result<AccountProfile, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profile = create_profile(input)?;
    let _ = activity_log::append_event(
        &profile.id,
        &profile.display_name,
        "success",
        "Account profile created",
    );
    Ok(profile)
}

#[tauri::command]
pub fn rename_account_profile(
    state: tauri::State<'_, AppState>,
    input: RenameAccountProfileInput,
) -> Result<AccountProfile, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profile = rename_profile(input)?;
    let _ = activity_log::append_event(
        &profile.id,
        &profile.display_name,
        "info",
        "Account profile renamed",
    );
    Ok(profile)
}

#[tauri::command]
pub fn remove_account_profile(
    state: tauri::State<'_, AppState>,
    input: RemoveAccountProfileInput,
) -> Result<(), String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profile_id = input.id.clone();
    remove_profile(input)?;
    sync_engine::on_agent_state_changed(&state, &profile_id, "idle")?;
    let _ = activity_log::append_event(&profile_id, "account", "warning", "Account profile removed");
    Ok(())
}

#[tauri::command]
pub fn set_account_agent_state(
    state: tauri::State<'_, AppState>,
    input: SetAccountAgentStateInput,
) -> Result<AccountProfile, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profile = set_agent_state(input)?;
    sync_engine::on_agent_state_changed(&state, &profile.id, &profile.agent_state)?;
    let message = format!("Agent state changed to {}", profile.agent_state);
    let _ = activity_log::append_event(&profile.id, &profile.display_name, "info", &message);
    Ok(profile)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountSyncRootInput {
    pub id: String,
    pub sync_root: String,
}

#[tauri::command]
pub fn set_account_sync_root(
    state: tauri::State<'_, AppState>,
    input: SetAccountSyncRootInput,
) -> Result<AccountProfile, String> {
    let path = std::path::PathBuf::from(input.sync_root.trim());
    if !path.is_absolute() {
        return Err("Sync root must be an absolute path".to_string());
    }

    fs::create_dir_all(&path)
        .map_err(|error| format!("Failed to create sync root '{}': {}", path.display(), error))?;

    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;

    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|profile| profile.id == input.id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    profile.sync_root = path.to_string_lossy().to_string();
    let updated = profile.clone();
    crate::app::account_profiles::save_profiles(&profiles)?;

    let _ = activity_log::append_event(
        &updated.id,
        &updated.display_name,
        "info",
        "Sync root updated",
    );

    Ok(updated)
}

#[tauri::command]
pub fn pause_all_accounts(state: tauri::State<'_, AppState>) -> Result<u32, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;

    let mut profiles = load_profiles()?;
    let mut changed_ids: Vec<String> = Vec::new();
    let mut changed_profiles: Vec<(String, String)> = Vec::new();

    for profile in &mut profiles {
        if profile.agent_state == "syncing" {
            profile.agent_state = "paused".to_string();
            changed_ids.push(profile.id.clone());
            changed_profiles.push((profile.id.clone(), profile.display_name.clone()));
        }
    }

    if changed_ids.is_empty() {
        return Ok(0);
    }

    save_profiles(&profiles)?;

    for profile_id in &changed_ids {
        sync_engine::on_agent_state_changed(&state, profile_id, "paused")?;
    }

    for (profile_id, profile_name) in &changed_profiles {
        let _ = activity_log::append_event(profile_id, profile_name, "warning", "Synchronization paused");
    }

    let _ = activity_log::append_event(
        "global",
        "all-accounts",
        "warning",
        "Paused synchronization for all active accounts",
    );

    Ok(changed_ids.len() as u32)
}

#[tauri::command]
pub fn resume_all_accounts(state: tauri::State<'_, AppState>) -> Result<u32, String> {
    let _guard = state
        .profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;

    let mut profiles = load_profiles()?;
    let mut changed_ids: Vec<String> = Vec::new();
    let mut changed_profiles: Vec<(String, String)> = Vec::new();

    for profile in &mut profiles {
        if profile.agent_state == "paused" {
            profile.agent_state = "syncing".to_string();
            changed_ids.push(profile.id.clone());
            changed_profiles.push((profile.id.clone(), profile.display_name.clone()));
        }
    }

    if changed_ids.is_empty() {
        return Ok(0);
    }

    save_profiles(&profiles)?;

    for profile_id in &changed_ids {
        sync_engine::on_agent_state_changed(&state, profile_id, "syncing")?;
    }

    for (profile_id, profile_name) in &changed_profiles {
        let _ = activity_log::append_event(profile_id, profile_name, "info", "Synchronization resumed");
    }

    let _ = activity_log::append_event(
        "global",
        "all-accounts",
        "info",
        "Resumed synchronization for paused accounts",
    );

    Ok(changed_ids.len() as u32)
}
