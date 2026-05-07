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

