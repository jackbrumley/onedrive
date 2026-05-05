use crate::app::account_profiles::{load_profiles, save_profiles};
use crate::app::activity_log;
use crate::app::state::{AppState, DeviceAuthPending, InteractiveAuthPending};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

const DEFAULT_MS_CLIENT_ID: &str = "ab9b8c07-8f02-4f72-87fa-80105867a763";
const DEFAULT_INTERACTIVE_CLIENT_ID: &str = "d50ca740-c83f-4d1b-b616-12c519384f0c";
const DEFAULT_SCOPE: &str = "offline_access Files.ReadWrite.All User.Read openid profile email";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const INTERACTIVE_AUTHORIZE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const INTERACTIVE_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const INTERACTIVE_REDIRECT_URI: &str = "https://login.microsoftonline.com/common/oauth2/nativeclient";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSessionSnapshot {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub updated_at: String,
}

#[tauri::command]
pub async fn start_device_auth(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<DeviceAuthSession, String> {
    log::info!("Starting device auth for profile_id={}", profile_id);
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

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed reading device code response body: {error}"))?;

    if !status.is_success() {
        let snippet: String = body.chars().take(400).collect();
        return Err(format!(
            "Device code request failed with status {status}. Response: {snippet}"
        ));
    }

    let payload: DeviceCodeResponse = serde_json::from_str(&body).map_err(|error| {
        let snippet: String = body.chars().take(400).collect();
        format!(
            "Failed to decode device code response: {error}. Response snippet: {snippet}"
        )
    })?;

    log::info!(
        "Device auth session created for profile_id={} verification_uri={} expires_in={}s",
        profile_id,
        payload.verification_uri,
        payload.expires_in
    );

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
    log::info!("Polling device auth token for profile_id={}", profile_id);
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

    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed reading token response body: {error}"))?;

    let payload: TokenResponse = serde_json::from_str(&body).map_err(|error| {
        let snippet: String = body.chars().take(400).collect();
        format!("Failed to decode token response: {error}. Response snippet: {snippet}")
    })?;

    if let Some(error) = payload.error.clone() {
        if error == "authorization_pending" || error == "slow_down" {
            log::info!("Device auth pending for profile_id={} status={}", profile_id, error);
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
    log::info!("Device auth completed for profile_id={}", profile_id);

    Ok(DeviceAuthPollResult {
        status: "authorized".to_string(),
        detail: "Authentication complete".to_string(),
    })
}

#[tauri::command]
pub async fn start_interactive_auth(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<(), String> {
    let interactive_client_id = resolve_interactive_client_id();
    let account_kind = {
        let _guard = state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let profiles = load_profiles()?;
        profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .map(|profile| profile.kind.clone())
            .ok_or_else(|| "Account profile not found".to_string())?
    };

    let domain_hint = domain_hint_for_kind(&account_kind)?;
    let authorize_url = INTERACTIVE_AUTHORIZE_URL.to_string();
    let token_url = INTERACTIVE_TOKEN_URL.to_string();
    let redirect_uri = INTERACTIVE_REDIRECT_URI.to_string();

    {
        log::info!(
            "Interactive auth routing for profile_id={} kind={} domain_hint={} client_id={} authorize_url={} token_url={} redirect_uri={}",
            profile_id,
            account_kind,
            domain_hint,
            interactive_client_id,
            authorize_url,
            token_url,
            redirect_uri
        );
    }

    let window_label = sanitize_auth_window_label(&profile_id);
    let state_token = generate_state_token();
    let authorize_request_url = format!(
        "{}?client_id={}&scope={}&response_type=code&prompt=login&redirect_uri={}&state={}&domain_hint={}",
        authorize_url,
        interactive_client_id,
        url_encode(DEFAULT_SCOPE),
        url_encode(&redirect_uri),
        url_encode(&state_token),
        url_encode(domain_hint)
    );

    {
        let mut pending = state
            .interactive_auth_pending
            .lock()
            .map_err(|_| "Interactive auth lock is poisoned".to_string())?;
        pending.insert(
            profile_id.clone(),
            InteractiveAuthPending {
                profile_id: profile_id.clone(),
                state: state_token.clone(),
                window_label: window_label.clone(),
                client_id: interactive_client_id.to_string(),
                redirect_uri: redirect_uri.clone(),
                token_url: token_url.clone(),
            },
        );
    }

    if let Some(existing_window) = app.get_webview_window(&window_label) {
        let _ = existing_window.close();
    }

    log::info!("Opening interactive auth window for profile_id={}", profile_id);

    let callback_profile_id = profile_id.clone();
    let callback_window_label = window_label.clone();
    let callback_app = app.clone();

    WebviewWindowBuilder::new(
        &app,
        &window_label,
        WebviewUrl::External(
            authorize_request_url
                .parse()
                .map_err(|error| format!("Failed to parse auth URL: {error}"))?,
        ),
    )
    .title("Microsoft Sign-In")
    .inner_size(980.0, 760.0)
    .resizable(true)
    .center()
    .on_navigation(move |url| {
        let as_text = url.as_str();
        if !as_text.starts_with(&redirect_uri) {
            return true;
        }

        let app_handle = callback_app.clone();
        let profile_id = callback_profile_id.clone();
        let window_label = callback_window_label.clone();
        let callback = as_text.to_string();

        tauri::async_runtime::spawn(async move {
            match complete_interactive_auth_from_callback(&app_handle, &profile_id, &callback).await {
                Ok(()) => {
                    log::info!("Interactive auth completed for profile_id={}", profile_id);
                }
                Err(error) => {
                    log::error!(
                        "Interactive auth failed for profile_id={}: {}",
                        profile_id,
                        error
                    );
                    let _ = activity_log::append_event(
                        &profile_id,
                        "account",
                        "error",
                        &format!("Interactive authentication failed: {error}"),
                    );
                }
            }

            if let Some(window) = app_handle.get_webview_window(&window_label) {
                let _ = window.close();
            }
        });

        false
    })
    .build()
    .map_err(|error| format!("Failed to open interactive sign-in window: {error}"))?;

    Ok(())
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

async fn complete_interactive_auth_from_callback(
    app: &tauri::AppHandle,
    profile_id: &str,
    callback_url: &str,
) -> Result<(), String> {
    let callback = callback_url
        .parse::<tauri::Url>()
        .map_err(|error| format!("Invalid callback URL: {error}"))?;

    let mut code: Option<String> = None;
    let mut returned_state: Option<String> = None;
    let mut oauth_error: Option<String> = None;
    let mut oauth_error_description: Option<String> = None;

    for (key, value) in callback.query_pairs() {
        let key_text = key.as_ref();
        if key_text == "code" {
            code = Some(value.to_string());
        } else if key_text == "state" {
            returned_state = Some(value.to_string());
        } else if key_text == "error" {
            oauth_error = Some(value.to_string());
        } else if key_text == "error_description" {
            oauth_error_description = Some(value.to_string());
        }
    }

    let app_state = app.state::<AppState>();

    let pending = {
        let lock = app_state
            .interactive_auth_pending
            .lock()
            .map_err(|_| "Interactive auth lock is poisoned".to_string())?;
        lock.get(profile_id)
            .map(|entry| {
                (
                    entry.profile_id.clone(),
                    entry.state.clone(),
                    entry.window_label.clone(),
                    entry.client_id.clone(),
                    entry.redirect_uri.clone(),
                    entry.token_url.clone(),
                )
            })
            .ok_or_else(|| "No pending interactive auth session for this profile".to_string())?
    };

    if !callback_url.starts_with(&pending.4) {
        clear_pending_interactive_auth(app, profile_id)?;
        return Err(format!(
            "OAuth callback redirect mismatch. Expected prefix '{}'",
            pending.4
        ));
    }

    if let Some(error_code) = oauth_error {
        let detail = oauth_error_description.unwrap_or_else(|| "Unknown OAuth error".to_string());
        clear_pending_interactive_auth(app, profile_id)?;
        return Err(format!("OAuth sign-in failed: {error_code} ({detail})"));
    }

    let state_value = returned_state.ok_or_else(|| "OAuth callback missing state".to_string())?;
    if state_value != pending.1 {
        clear_pending_interactive_auth(app, profile_id)?;
        return Err("OAuth callback state mismatch".to_string());
    }

    let authorization_code = code.ok_or_else(|| "OAuth callback missing authorization code".to_string())?;

    let client = reqwest::Client::new();
    let response = client
        .post(&pending.5)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", pending.3.as_str()),
            ("code", authorization_code.as_str()),
            ("redirect_uri", pending.4.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("Failed to exchange authorization code: {error}"))?;

    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed reading authorization token response body: {error}"))?;

    let payload: TokenResponse = serde_json::from_str(&body).map_err(|error| {
        let snippet: String = body.chars().take(400).collect();
        format!(
            "Failed to decode authorization token response: {error}. Response snippet: {snippet}"
        )
    })?;

    if let Some(error) = payload.error.clone() {
        clear_pending_interactive_auth(app, profile_id)?;
        return Err(payload
            .error_description
            .unwrap_or_else(|| format!("Authentication failed: {error}")));
    }

    let access_token = payload
        .access_token
        .ok_or_else(|| "Token response missing access token".to_string())?;
    let updated_at = chrono::Local::now().to_rfc3339();

    write_auth_session(
        profile_id,
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
        let _guard = app_state
            .profiles_lock
            .lock()
            .map_err(|_| "Account profile lock is poisoned".to_string())?;
        let mut profiles = load_profiles()?;
        if let Some(profile) = profiles.iter_mut().find(|profile| profile.id == profile_id) {
            profile.auth_configured = true;
        }
        save_profiles(&profiles)?;
    }

    clear_pending_interactive_auth(app, profile_id)?;

    let _ = activity_log::append_event(
        profile_id,
        "account",
        "success",
        "Interactive authentication completed successfully",
    );

    Ok(())
}

fn clear_pending_interactive_auth(app: &tauri::AppHandle, profile_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut lock = state
        .interactive_auth_pending
        .lock()
        .map_err(|_| "Interactive auth lock is poisoned".to_string())?;
    lock.remove(profile_id);
    Ok(())
}

fn domain_hint_for_kind(kind: &str) -> Result<&'static str, String> {
    match kind.trim().to_lowercase().as_str() {
        "personal" => Ok("consumers"),
        "business" => Ok("organizations"),
        _ => Err("Unsupported account kind for interactive auth".to_string()),
    }
}

fn resolve_interactive_client_id() -> String {
    std::env::var("ONEDRIVE_INTERACTIVE_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_CLIENT_ID.to_string())
}

fn sanitize_auth_window_label(profile_id: &str) -> String {
    let mut label = String::from("auth-");
    for character in profile_id.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            label.push(character);
        }
    }
    if label == "auth-" {
        label.push_str("profile");
    }
    label
}

fn generate_state_token() -> String {
    let now = chrono::Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_default();
    let pid = std::process::id();
    format!("s{}{}", now, pid)
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        let should_encode = !matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if should_encode {
            encoded.push('%');
            encoded.push_str(&format!("{:02X}", byte));
        } else {
            encoded.push(byte as char);
        }
    }
    encoded
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

pub fn load_auth_session(profile_id: &str) -> Result<AuthSessionSnapshot, String> {
    let file_path = auth_session_file_path(profile_id)?;
    if !file_path.exists() {
        return Err("Account auth session not found".to_string());
    }
    let text = fs::read_to_string(file_path).map_err(|error| error.to_string())?;
    let session: StoredAuthSession = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(AuthSessionSnapshot {
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        expires_in: session.expires_in,
        token_type: session.token_type,
        scope: session.scope,
        updated_at: session.updated_at,
    })
}

pub async fn refresh_access_token(profile_id: &str) -> Result<AuthSessionSnapshot, String> {
    let existing = load_auth_session(profile_id)?;
    let refresh_token = existing
        .refresh_token
        .clone()
        .ok_or_else(|| "Refresh token not available; re-authentication required".to_string())?;

    let client_id = resolve_interactive_client_id();
    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("scope", DEFAULT_SCOPE),
        ])
        .send()
        .await
        .map_err(|error| format!("Failed refreshing access token: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed reading refresh-token response body: {error}"))?;

    let payload: TokenResponse = serde_json::from_str(&body).map_err(|error| {
        let snippet: String = body.chars().take(400).collect();
        format!(
            "Failed to decode refresh-token response: {error}. Response snippet: {snippet}"
        )
    })?;

    if !status.is_success() || payload.error.is_some() {
        let detail = payload
            .error_description
            .unwrap_or_else(|| "Unknown token refresh error".to_string());
        let code = payload.error.unwrap_or_else(|| status.to_string());
        return Err(format!("Token refresh failed: {code} ({detail})"));
    }

    let access_token = payload
        .access_token
        .ok_or_else(|| "Refresh response missing access token".to_string())?;
    let next_refresh_token = payload.refresh_token.or(existing.refresh_token);
    let updated_at = chrono::Local::now().to_rfc3339();

    write_auth_session(
        profile_id,
        StoredAuthSession {
            access_token: access_token.clone(),
            refresh_token: next_refresh_token.clone(),
            expires_in: payload.expires_in,
            token_type: payload.token_type.clone(),
            scope: payload.scope.clone(),
            updated_at: updated_at.clone(),
        },
    )?;

    log::info!("Access token refreshed for profile_id={}", profile_id);

    Ok(AuthSessionSnapshot {
        access_token,
        refresh_token: next_refresh_token,
        expires_in: payload.expires_in,
        token_type: payload.token_type,
        scope: payload.scope,
        updated_at,
    })
}
