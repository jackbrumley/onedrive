use crate::app::account_profiles::{load_profiles, save_profiles};
use crate::app::activity_log;
use crate::app::state::{AppState, DeviceAuthPending};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_MS_CLIENT_ID: &str = "ab9b8c07-8f02-4f72-87fa-80105867a763";
const DEFAULT_SCOPE: &str = "offline_access Files.ReadWrite.All User.Read openid profile email";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthSession {
    profile_id: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthPollResult {
    status: String,
    detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAuthSession {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    token_type: Option<String>,
    scope: Option<String>,
    updated_at: String,
}

#[tauri::command]
pub async fn start_device_auth(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<DeviceAuthSession, String> {
    {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let profiles = load_profiles()?;
        if !profiles.iter().any(|profile| profile.id == profile_id) {
            return Err("Account profile not found".to_string());
        }
    }

    let client = reqwest::Client::new();
    let response = client
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", DEFAULT_MS_CLIENT_ID), ("scope", DEFAULT_SCOPE)])
        .send()
        .await
        .map_err(|error| format!("Failed to request device code: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("Device code request failed with status {}", response.status()));
    }

    let payload: DeviceCodeResponse = response
        .json()
        .await
        .map_err(|error| format!("Failed to decode device code response: {error}"))?;

    {
        let mut pending = state
            .auth_pending
            .lock()
            .map_err(|_| "Auth pending lock is poisoned".to_string())?;
        pending.insert(
            profile_id.clone(),
            DeviceAuthPending {
                profile_id: profile_id.clone(),
                device_code: payload.device_code.clone(),
                interval_secs: payload.interval.unwrap_or(5),
            },
        );
    }

    Ok(DeviceAuthSession {
        profile_id,
        user_code: payload.user_code,
        verification_uri: payload.verification_uri,
        verification_uri_complete: payload.verification_uri_complete,
        expires_in: payload.expires_in,
        interval: payload.interval.unwrap_or(5),
        message: payload.message,
    })
}

#[tauri::command]
pub async fn poll_device_auth(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<DeviceAuthPollResult, String> {
    let pending = {
        let lock = state
            .auth_pending
            .lock()
            .map_err(|_| "Auth pending lock is poisoned".to_string())?;
        lock.get(&profile_id)
            .map(|entry| (entry.profile_id.clone(), entry.device_code.clone(), entry.interval_secs))
            .ok_or_else(|| "No active auth session for this profile".to_string())?
    };

    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", DEFAULT_MS_CLIENT_ID),
            ("device_code", pending.1.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("Failed to poll token endpoint: {error}"))?;

    let payload: TokenResponse = response
        .json()
        .await
        .map_err(|error| format!("Failed to decode token response: {error}"))?;

    if let Some(error) = payload.error.clone() {
        if error == "authorization_pending" || error == "slow_down" {
            return Ok(DeviceAuthPollResult {
                status: "pending".to_string(),
                detail: payload
                    .error_description
                    .unwrap_or_else(|| "Waiting for user approval".to_string()),
            });
        }

        {
            let mut lock = state
                .auth_pending
                .lock()
                .map_err(|_| "Auth pending lock is poisoned".to_string())?;
            lock.remove(&profile_id);
        }

        return Ok(DeviceAuthPollResult {
            status: "error".to_string(),
            detail: payload
                .error_description
                .unwrap_or_else(|| format!("Authentication failed: {error}")),
        });
    }

    let access_token = payload
        .access_token
        .ok_or_else(|| "Token response missing access token".to_string())?;
    let updated_at = chrono::Local::now().to_rfc3339();

    write_auth_session(
        &profile_id,
        StoredAuthSession {
            access_token,
            refresh_token: payload.refresh_token,
            expires_in: payload.expires_in,
            token_type: payload.token_type,
            scope: payload.scope,
            updated_at,
        },
    )?;

    {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let mut profiles = load_profiles()?;
        if let Some(profile) = profiles.iter_mut().find(|profile| profile.id == profile_id) {
            profile.auth_configured = true;
        }
        save_profiles(&profiles)?;
    }

    {
        let mut lock = state
            .auth_pending
            .lock()
            .map_err(|_| "Auth pending lock is poisoned".to_string())?;
        lock.remove(&profile_id);
    }

    let _ = activity_log::append_event(
        &profile_id,
        "account",
        "success",
        "Authentication completed successfully",
    );

    Ok(DeviceAuthPollResult {
        status: "authorized".to_string(),
        detail: "Authentication complete".to_string(),
    })
}

#[tauri::command]
pub fn clear_account_auth(state: tauri::State<'_, AppState>, profile_id: String) -> Result<(), String> {
    {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let mut profiles = load_profiles()?;
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "Account profile not found".to_string())?;
        profile.auth_configured = false;
        save_profiles(&profiles)?;
    }

    let auth_path = auth_session_file_path(&profile_id)?;
    if auth_path.exists() {
        fs::remove_file(auth_path).map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn auth_session_file_path(profile_id: &str) -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir
        .join("onedrive")
        .join("accounts")
        .join(profile_id)
        .join("auth.json"))
}

fn write_auth_session(profile_id: &str, session: StoredAuthSession) -> Result<(), String> {
    let file_path = auth_session_file_path(profile_id)?;
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&session).map_err(|error| error.to_string())?;
    fs::write(file_path, text).map_err(|error| error.to_string())
}
