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
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 12;
const MAX_DOWNLOAD_RETRIES: u32 = 5;
const MAX_RETRY_DELAY_SECONDS: u64 = 30;
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 60;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 15;
const DEFAULT_DOWNLOAD_TIMEOUT_BASE_SECONDS: u64 = 30;
const DEFAULT_DOWNLOAD_TIMEOUT_MIN_SECONDS: u64 = 60;
const DEFAULT_DOWNLOAD_TIMEOUT_MAX_SECONDS: u64 = 1800;
const DEFAULT_DOWNLOAD_TIMEOUT_PER_MIB_MILLIS: u64 = 1400;
const DEFAULT_STALL_TIMEOUT_SECONDS: u64 = 60;
const SYNC_CANCELLED_ERROR: &str = "Synchronization cancelled";
const DOWNLOAD_RETRY_DEFERRED_ERROR: &str = "Download retry deferred";
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
    if let Err(error) = persist_sync_lifecycle_agent_state(profile_id, agent_state) {
        log::warn!(
            "{} SYNC_LIFECYCLE_AGENT_STATE_PERSIST_FAILED state={} error={}",
            log_context::account_prefix(profile_id),
            agent_state,
            error
        );
    }
    if agent_state == "syncing" {
        let _ = set_cancel_flag(state, profile_id, false)?;
        runtime_clear_issue(&state.sync_runtime, profile_id);
        start_sync_worker(state, profile_id)?;
    } else {
        let _ = set_cancel_flag(state, profile_id, true)?;
        stop_sync_worker(state, profile_id)?;
        match reset_running_sync_jobs_for_pause(profile_id) {
            Ok(cleared_jobs) => {
                if cleared_jobs > 0 {
                    log::info!(
                        "{} SYNC_PAUSE_DRAINED running_jobs_cleared={}",
                        log_context::account_prefix(profile_id),
                        cleared_jobs
                    );
                }
            }
            Err(error) => {
                log::warn!(
                    "{} SYNC_PAUSE_DRAIN_FAILED error={}",
                    log_context::account_prefix(profile_id),
                    error
                );
            }
        }
        runtime_reset_transfer_activity(&state.sync_runtime, profile_id);
        let (phase, message) = if agent_state == "paused" {
            ("paused", "Synchronization paused")
        } else {
            ("idle", "Idle")
        };
        runtime_set_phase(&state.sync_runtime, profile_id, phase, message);
        runtime_clear_issue(&state.sync_runtime, profile_id);
    }
    Ok(())
}

pub fn prepare_startup_sync_resume(profile_id: &str) -> Result<usize, String> {
    let cleared_jobs = reset_running_sync_jobs_for_pause(profile_id)?;
    if cleared_jobs > 0 {
        log::info!(
            "{} STARTUP_SYNC_PREP_DRAINED running_jobs_cleared={}",
            log_context::account_prefix(profile_id),
            cleared_jobs
        );
    }
    Ok(cleared_jobs)
}

pub fn confirm_large_delete_guard(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
) -> Result<(), String> {
    let mut sync_state = load_sync_state(profile_id)?;
    if sync_state.large_delete_pending_paths.is_empty() {
        return Err("No pending large deletion to confirm".to_string());
    }
    sync_state.large_delete_guard_approved = true;
    save_sync_state(profile_id, &sync_state)?;

    runtime_clear_issue(&state.sync_runtime, profile_id);
    runtime_set_phase(
        &state.sync_runtime,
        profile_id,
        "applying_local",
        "Large deletion confirmed - applying changes",
    );
    Ok(())
}

pub fn keep_cloud_files_after_large_delete(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
) -> Result<(), String> {
    let mut sync_state = load_sync_state(profile_id)?;
    sync_state.large_delete_guard_approved = false;
    sync_state.large_delete_pending_paths.clear();
    sync_state.two_way_ready = false;
    sync_state.bootstrap_scan_initialized = false;
    sync_state.bootstrap_full_scan_completed = false;
    sync_state.delta_link = None;
    sync_state.active_delta_next_link = None;
    save_sync_state(profile_id, &sync_state)?;

    runtime_clear_issue(&state.sync_runtime, profile_id);
    runtime_set_phase(
        &state.sync_runtime,
        profile_id,
        "syncing",
        "Initial sync in progress - downloading cloud files only",
    );
    Ok(())
}

pub fn get_large_delete_pending_paths(profile_id: &str) -> Result<Vec<String>, String> {
    let sync_state = load_sync_state(profile_id)?;
    Ok(sync_state.large_delete_pending_paths)
}
