use crate::app::account_profiles::{
    create_profile, load_profiles, remove_profile, rename_profile, save_profiles, set_agent_state,
    AccountProfile, CreateAccountProfileInput, RemoveAccountProfileInput,
    RenameAccountProfileInput, SetAccountAgentStateInput,
};
use crate::app::activity_log;
use crate::app::log_context;
use crate::app::state::AppState;
use crate::app::sync_engine;
use crate::app::sync_runtime;
use std::fs;
use std::path::{Component, PathBuf};
use std::process::Command;

#[tauri::command]
pub fn list_account_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AccountProfile>, String> {
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
        &profile.email,
        "success",
        &format!(
            "{} Account profile created",
            log_context::account_prefix_from_parts(&profile.id, &profile.email)
        ),
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
        &profile.email,
        "info",
        &format!(
            "{} Account profile renamed",
            log_context::account_prefix_from_parts(&profile.id, &profile.email)
        ),
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
    if let Ok(mut runtime_map) = state.sync_runtime.lock() {
        sync_runtime::remove_account(&mut runtime_map, &profile_id);
    }
    let _ = activity_log::append_event(
        &profile_id,
        &profile_id,
        "warning",
        &format!(
            "{} Account profile removed",
            log_context::account_prefix(&profile_id)
        ),
    );
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
    log::info!(
        "{} SYNC_AGENT_STATE_SET state={}",
        log_context::account_prefix_from_parts(&profile.id, &profile.email),
        profile.agent_state
    );
    sync_engine::on_agent_state_changed(&state, &profile.id, &profile.agent_state)?;
    let message = format!(
        "{} Agent state changed to {}",
        log_context::account_prefix_from_parts(&profile.id, &profile.email),
        profile.agent_state
    );
    let _ = activity_log::append_event(&profile.id, &profile.email, "info", &message);
    Ok(profile)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountSyncRootInput {
    pub id: String,
    pub sync_root: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAccountItemFolderInput {
    pub profile_id: String,
    pub relative_path: String,
}

#[tauri::command]
pub fn open_account_sync_root_folder(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<(), String> {
    let profile = {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let profiles = load_profiles()?;
        profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "Account profile not found".to_string())?
    };

    let folder_path = PathBuf::from(profile.sync_root);
    fs::create_dir_all(&folder_path).map_err(|error| {
        format!(
            "Failed to create folder '{}': {}",
            folder_path.display(),
            error
        )
    })?;

    open_folder_in_file_manager(&folder_path)
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
        &updated.email,
        "info",
        &format!(
            "{} Sync root updated",
            log_context::account_prefix_from_parts(&updated.id, &updated.email)
        ),
    );

    Ok(updated)
}

#[tauri::command]
pub fn open_account_item_folder(
    state: tauri::State<'_, AppState>,
    input: OpenAccountItemFolderInput,
) -> Result<(), String> {
    let profile = {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let profiles = load_profiles()?;
        profiles
            .into_iter()
            .find(|profile| profile.id == input.profile_id)
            .ok_or_else(|| "Account profile not found".to_string())?
    };

    let relative_path = normalize_relative_sync_item_path(&input.relative_path)?;
    let sync_root = PathBuf::from(profile.sync_root);
    let absolute_path = sync_root.join(relative_path);
    let folder_path = absolute_path.parent().unwrap_or(&sync_root).to_path_buf();

    fs::create_dir_all(&folder_path).map_err(|error| {
        format!(
            "Failed to create folder '{}': {}",
            folder_path.display(),
            error
        )
    })?;

    open_folder_in_file_manager(&folder_path)
}

fn normalize_relative_sync_item_path(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Sync path is empty".to_string());
    }
    let input = PathBuf::from(trimmed);
    if input.is_absolute() {
        return Err("Sync path must be relative".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("Sync path contains invalid segments".to_string())
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err("Sync path is empty".to_string());
    }

    Ok(normalized)
}

fn open_folder_in_file_manager(folder_path: &PathBuf) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(folder_path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(folder_path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(folder_path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
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
            changed_profiles.push((profile.id.clone(), profile.email.clone()));
        }
    }

    if changed_ids.is_empty() {
        return Ok(0);
    }

    save_profiles(&profiles)?;

    for profile_id in &changed_ids {
        sync_engine::on_agent_state_changed(&state, profile_id, "paused")?;
    }

    for (profile_id, profile_email) in &changed_profiles {
        let _ = activity_log::append_event(
            profile_id,
            profile_email,
            "warning",
            &format!(
                "{} Synchronization paused",
                log_context::account_prefix_from_parts(profile_id, profile_email)
            ),
        );
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
            changed_profiles.push((profile.id.clone(), profile.email.clone()));
        }
    }

    if changed_ids.is_empty() {
        return Ok(0);
    }

    save_profiles(&profiles)?;

    for profile_id in &changed_ids {
        sync_engine::on_agent_state_changed(&state, profile_id, "syncing")?;
    }

    for (profile_id, profile_email) in &changed_profiles {
        let _ = activity_log::append_event(
            profile_id,
            profile_email,
            "info",
            &format!(
                "{} Synchronization resumed",
                log_context::account_prefix_from_parts(profile_id, profile_email)
            ),
        );
    }

    let _ = activity_log::append_event(
        "global",
        "all-accounts",
        "info",
        "Resumed synchronization for paused accounts",
    );

    Ok(changed_ids.len() as u32)
}
