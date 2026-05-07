use crate::app::account_profiles::{load_profiles, save_profiles, AccountProfile};
use crate::app::activity_log;
use crate::app::auth::{load_auth_session, refresh_access_token};
use crate::app::log_context;
use crate::app::state::AppState;
use crate::app::sync_runtime::{self, SyncRuntimeMap};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 8;
const MAX_DOWNLOAD_CONCURRENCY: usize = 32;
const MAX_DOWNLOAD_RETRIES: u32 = 5;
const MAX_RETRY_DELAY_SECONDS: u64 = 30;
const SYNC_CANCELLED_ERROR: &str = "Synchronization cancelled";
const CANCEL_POLL_INTERVAL_MILLIS: u64 = 100;

pub fn on_agent_state_changed(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
    agent_state: &str,
) -> Result<(), String> {
    log::info!(
        "{} SYNC_AGENT_STATE_CHANGE requested_state={}",
        log_context::account_prefix(profile_id),
        agent_state
    );
    if agent_state == "syncing" {
        let _ = set_cancel_flag(state, profile_id, false)?;
        runtime_clear_issue(&state.sync_runtime, profile_id);
        start_sync_worker(state, profile_id)?;
    } else {
        let _ = set_cancel_flag(state, profile_id, true)?;
        stop_sync_worker(state, profile_id)?;
        if let Ok(mut runtime_map) = state.sync_runtime.lock() {
            sync_runtime::clear_in_progress(&mut runtime_map, profile_id);
            sync_runtime::clear_issue(&mut runtime_map, profile_id);
            let (phase, message) = if agent_state == "paused" {
                ("paused", "Synchronization paused")
            } else {
                ("idle", "Idle")
            };
            sync_runtime::set_phase(&mut runtime_map, profile_id, phase, message);
        }
    }
    Ok(())
}
