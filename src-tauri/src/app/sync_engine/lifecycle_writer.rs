fn runtime_set_phase(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    phase: &str,
    phase_message: &str,
) {
    let mut previous_phase: Option<String> = None;
    let mut previous_message: Option<String> = None;
    if let Ok(mut runtime_map) = runtime.lock() {
        if let Some(existing) = runtime_map.get(profile_id) {
            previous_phase = Some(existing.phase.clone());
            previous_message = Some(existing.phase_message.clone());
        }
        sync_runtime::set_phase(&mut runtime_map, profile_id, phase, phase_message);
    }
    let phase_changed = previous_phase.as_deref() != Some(phase)
        || previous_message.as_deref() != Some(phase_message);
    if phase_changed {
        log::info!(
            "{} PHASE_TRANSITION from_phase={} from_message={} to_phase={} to_message={}",
            log_context::account_prefix(profile_id),
            previous_phase.as_deref().unwrap_or("(none)"),
            previous_message.as_deref().unwrap_or("(none)"),
            phase,
            phase_message
        );
    }
    if let Err(error) = persist_sync_lifecycle_phase(profile_id, phase, phase_message) {
        log::warn!(
            "{} SYNC_LIFECYCLE_PHASE_PERSIST_FAILED phase={} error={}",
            log_context::account_prefix(profile_id),
            phase,
            error
        );
    }
}

pub fn runtime_set_profile_auth_ready(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    auth_ready: bool,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_auth_ready(&mut runtime_map, profile_id, auth_ready);
    }
}

pub fn runtime_set_engine_state(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    engine_state: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_engine_state(&mut runtime_map, profile_id, engine_state);
    }
}

fn runtime_reset_transfer_activity(runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>, profile_id: &str) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::clear_in_progress(&mut runtime_map, profile_id);
        sync_runtime::set_remote_transfer_progress(&mut runtime_map, profile_id, 0, 0, 0);
    }
}

fn runtime_set_local_scan_progress(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    scanned_count: usize,
    estimated_total: Option<usize>,
    current_path: Option<&str>,
    cycle_id: Option<&str>,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_local_scan_progress(
            &mut runtime_map,
            profile_id,
            scanned_count,
            estimated_total,
            current_path,
            cycle_id,
        );
    }
    if let Err(error) = persist_sync_lifecycle_activity(
        profile_id,
        "scanning_local",
        if estimated_total.is_some() {
            "determinate"
        } else {
            "indeterminate"
        },
        Some(scanned_count),
        estimated_total,
        Some("files"),
        current_path,
        cycle_id,
    ) {
        log::warn!(
            "{} SYNC_LIFECYCLE_ACTIVITY_PERSIST_FAILED stage=scanning_local error={}",
            log_context::account_prefix(profile_id),
            error
        );
    }
}

fn runtime_set_current_activity(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    stage: &str,
    progress_mode: &str,
    current: Option<usize>,
    total: Option<usize>,
    unit: Option<&str>,
    detail: Option<&str>,
    cycle_id: Option<&str>,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_current_activity(
            &mut runtime_map,
            profile_id,
            stage,
            progress_mode,
            current,
            total,
            unit,
            detail,
            cycle_id,
        );
    }
    if let Err(error) = persist_sync_lifecycle_activity(
        profile_id,
        stage,
        progress_mode,
        current,
        total,
        unit,
        detail,
        cycle_id,
    ) {
        log::warn!(
            "{} SYNC_LIFECYCLE_ACTIVITY_PERSIST_FAILED stage={} error={}",
            log_context::account_prefix(profile_id),
            stage,
            error
        );
    }
}

fn runtime_clear_issue(runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>, profile_id: &str) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::clear_issue(&mut runtime_map, profile_id);
    }
    if let Err(error) = clear_persisted_sync_issue(profile_id) {
        log::warn!(
            "{} SYNC_ISSUE_CLEAR_PERSIST_FAILED error={}",
            log_context::account_prefix(profile_id),
            error
        );
    }
}

fn runtime_set_issue(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    issue_code: &str,
    issue_message: &str,
    issue_actions: &[&str],
    issue_path: Option<&str>,
    issue_secondary_path: Option<&str>,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_issue(
            &mut runtime_map,
            profile_id,
            issue_code,
            issue_message,
            issue_actions,
            issue_path,
            issue_secondary_path,
        );
    }
    if let Err(error) = persist_sync_issue(
        profile_id,
        issue_code,
        issue_message,
        issue_actions,
        issue_path,
        issue_secondary_path,
    ) {
        log::warn!(
            "{} SYNC_ISSUE_PERSIST_FAILED code={} error={}",
            log_context::account_prefix(profile_id),
            issue_code,
            error
        );
    }
}

fn runtime_set_remote_scan_complete(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    complete: bool,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_remote_scan_complete(&mut runtime_map, profile_id, complete);
    }
    if let Err(error) = persist_sync_lifecycle_remote_scan_complete(profile_id, complete) {
        log::warn!(
            "{} SYNC_LIFECYCLE_SCAN_COMPLETE_PERSIST_FAILED complete={} error={}",
            log_context::account_prefix(profile_id),
            complete,
            error
        );
    }
}

fn runtime_record_remote_discovered(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    item_id: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::record_remote_discovered(&mut runtime_map, profile_id, item_id);
    }
}

fn runtime_record_remote_download_planned(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    item_id: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::record_remote_download_planned(&mut runtime_map, profile_id, item_id);
    }
}

fn runtime_record_remote_download_completed(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    item_id: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::record_remote_download_completed(&mut runtime_map, profile_id, item_id);
    }
}

fn runtime_record_remote_download_failed(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    item_id: &str,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::record_remote_download_failed(&mut runtime_map, profile_id, item_id);
    }
}

fn runtime_set_remote_download_counters(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    planned_total: usize,
    completed_total: usize,
    failed_total: usize,
    in_flight: usize,
    retry_waiting: usize,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_remote_download_counters(
            &mut runtime_map,
            profile_id,
            planned_total,
            completed_total,
            failed_total,
            in_flight,
            retry_waiting,
        );
    }
}

fn runtime_set_upload_counters(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    planned_total: usize,
    completed_total: usize,
    failed_total: usize,
    in_flight: usize,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_upload_counters(
            &mut runtime_map,
            profile_id,
            planned_total,
            completed_total,
            failed_total,
            in_flight,
        );
    }
}

fn runtime_set_upload_planned_total(
    runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    planned_total: usize,
) {
    if let Ok(mut runtime_map) = runtime.lock() {
        sync_runtime::set_upload_planned_total(&mut runtime_map, profile_id, planned_total);
    }
}

fn classify_sync_issue(error: &str) -> (&'static str, &'static [&'static str]) {
    let normalized = error.to_lowercase();
    if normalized.contains("re-authentication")
        || normalized.contains("not authenticated")
        || normalized.contains("access token is empty")
        || normalized.contains("401")
    {
        return ("auth_required", &["reauthenticate", "retry_sync"]);
    }
    if normalized.contains("429") || normalized.contains("too many requests") {
        return ("rate_limited", &["retry_sync"]);
    }
    if normalized.contains("permission denied") || normalized.contains("403") {
        return ("permission_denied", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("no space left") || normalized.contains("disk full") {
        return ("disk_full", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("sync root") {
        return ("sync_root_unavailable", &["open_sync_root", "retry_sync"]);
    }
    if normalized.contains("conflict") || normalized.contains("safebackup") {
        return (
            "conflict_detected",
            &["open_conflict", "open_sync_root", "retry_sync"],
        );
    }
    if normalized.contains("network")
        || normalized.contains("connection")
        || normalized.contains("timed out")
        || normalized.contains("dns")
    {
        return ("network_unavailable", &["retry_sync"]);
    }
    ("unknown_error", &["retry_sync"])
}

fn relative_path_for_issue(sync_root: &Path, candidate: &Path) -> Option<String> {
    let relative = candidate.strip_prefix(sync_root).ok()?;
    let mut output = String::new();
    for component in relative.components() {
        if let std::path::Component::Normal(segment) = component {
            if !output.is_empty() {
                output.push('/');
            }
            output.push_str(&segment.to_string_lossy());
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}
