use crate::app::account_profiles::{load_profiles, save_profiles};
use crate::app::activity_log;
use crate::app::log_context;
use crate::app::state::{AppState, DeviceAuthPending, InteractiveAuthPending};
use crate::app::sync_engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const DEFAULT_MS_CLIENT_ID: &str = "ab9b8c07-8f02-4f72-87fa-80105867a763";
const DEFAULT_INTERACTIVE_CLIENT_ID: &str = "d50ca740-c83f-4d1b-b616-12c519384f0c";
const DEFAULT_SCOPE: &str = "offline_access Files.ReadWrite.All User.Read openid profile email";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const INTERACTIVE_AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const INTERACTIVE_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const INTERACTIVE_REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";
const GRAPH_ME_URL: &str = "https://graph.microsoft.com/v1.0/me?$select=mail,userPrincipalName";

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMeResponse {
    mail: Option<String>,
    user_principal_name: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InteractiveAuthUpdatedEvent {
    profile_id: String,
    email: String,
    auth_configured: bool,
    agent_state: String,
}
