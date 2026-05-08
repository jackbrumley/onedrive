async fn tick_sync_cycle(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    sync_runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<SyncCycleStats, String> {
    ensure_not_cancelled(cancel_flag)?;
    let profile = load_syncable_profile(profiles_lock, profile_id)?;
    let account_prefix = log_context::account_prefix_from_parts(profile_id, &profile.email);
    let cycle_id = new_cycle_id();
    let sync_root = PathBuf::from(profile.sync_root.clone());
    std::fs::create_dir_all(&sync_root).map_err(|error| {
        format!(
            "Failed to create sync root '{}': {}",
            sync_root.display(),
            error
        )
    })?;

    let session = load_auth_session(profile_id)?;
    if session.access_token.trim().is_empty() {
        return Err("Auth access token is empty; re-authentication required".to_string());
    }

    let mut graph = GraphContext {
        profile_id: profile_id.to_string(),
        account_prefix: account_prefix.clone(),
        cycle_id: cycle_id.clone(),
        access_token: Arc::new(tokio::sync::RwLock::new(session.access_token)),
        token_refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        sync_runtime: Arc::clone(sync_runtime),
    };

    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "syncing",
        "Preparing synchronization cycle",
    );

    let mut sync_state = load_sync_state(profile_id)?;
    let mut stats = SyncCycleStats {
        account_prefix: account_prefix.clone(),
        cycle_id: cycle_id.clone(),
        ..SyncCycleStats::default()
    };
    log::info!(
        "{} [cycle:{}] SYNC_CYCLE_START sync_root={}",
        account_prefix,
        cycle_id,
        sync_root.display()
    );
    let _ = activity_log::append_event(
        profile_id,
        &profile.email,
        "info",
        &format!("{} [cycle:{}] SYNC_CYCLE_START", account_prefix, cycle_id),
    );

    let remote_applied_paths = fetch_and_apply_delta_changes(
        &mut graph,
        &sync_root,
        &mut sync_state,
        &mut stats,
        cancel_flag,
    )
    .await?;

    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "scanning_local",
        "Scanning local files",
    );
    let local_snapshot = collect_local_snapshot(&sync_root)?;
    rebuild_sync_file_index(profile_id, &sync_state, &local_snapshot)?;
    let planner_counters = recompute_sync_file_actions(profile_id, sync_state.two_way_ready)?;
    runtime_set_upload_planned_total(
        &graph.sync_runtime,
        profile_id,
        planner_counters.need_upload_total,
    );
    log::info!(
        "{} [cycle:{}] SYNC_PLANNER_SUMMARY cloud_discovered={} local_discovered={} need_download={} need_upload={} conflicts={}",
        account_prefix,
        cycle_id,
        planner_counters.cloud_discovered_total,
        planner_counters.local_discovered_total,
        planner_counters.need_download_total,
        planner_counters.need_upload_total,
        planner_counters.conflict_total,
    );
    stats.local_items_seen = local_snapshot.len();
    log::info!(
        "{} [cycle:{}] LOCAL_SCAN_SUMMARY items={}",
        account_prefix,
        cycle_id,
        stats.local_items_seen
    );
    if sync_state.two_way_ready {
        runtime_set_phase(
            &graph.sync_runtime,
            profile_id,
            "applying_local",
            "Applying local changes",
        );
        ensure_not_cancelled(cancel_flag)?;
        apply_local_changes(
            &mut graph,
            &sync_root,
            &local_snapshot,
            &remote_applied_paths,
            &mut sync_state,
            &mut stats,
            cancel_flag,
        )
        .await?;
    } else {
        let download_counters = read_download_job_counters(profile_id)?;
        let bootstrap_ready_for_two_way = sync_state.active_delta_next_link.is_none()
            && planner_counters.need_download_total == 0
            && download_counters.remaining == 0
            && download_counters.failed_terminal == 0;

        if bootstrap_ready_for_two_way {
            runtime_set_phase(
                &graph.sync_runtime,
                profile_id,
                "applying_local",
                "Preparing two-way sync - building your local baseline",
            );
            ensure_not_cancelled(cancel_flag)?;
            reconcile_bootstrap_local_snapshot(
                &mut graph,
                &sync_root,
                &local_snapshot,
                &mut sync_state,
                &mut stats,
                cancel_flag,
            )
            .await?;
            sync_state.two_way_ready = true;
        } else {
            runtime_set_phase(
                &graph.sync_runtime,
                profile_id,
                "syncing",
                "Initial sync in progress - downloading cloud files only",
            );
            log::warn!(
                "{} [cycle:{}] BOOTSTRAP_TWO_WAY_BLOCKED cursor_active={} planner_need_download={} queue_remaining={} failed_terminal={}",
                account_prefix,
                cycle_id,
                sync_state.active_delta_next_link.is_some(),
                planner_counters.need_download_total,
                download_counters.remaining,
                download_counters.failed_terminal
            );
        }
    }

    sync_state.local_snapshot = collect_local_snapshot(&sync_root)?;
    sync_state.last_cycle_at = Some(chrono::Local::now().to_rfc3339());
    save_sync_state(profile_id, &sync_state)?;

    update_profile_last_sync(profiles_lock, profile_id)?;

    let summary = format!(
        "Sync cycle complete (downloaded {}, uploaded {}, upload failures {}, upload cooldown skips {}, remote deletes {}, local deletes {}, remote pages {}, remote items {}, remote missing skips {}, local items {})",
        stats.downloaded_files,
        stats.uploaded_files,
        stats.upload_failures,
        stats.upload_cooldown_skips,
        stats.deleted_remote,
        stats.deleted_local,
        stats.remote_pages,
        stats.remote_items_received,
        stats.remote_items_skipped_missing,
        stats.local_items_seen
    );
    let _ = activity_log::append_event(
        profile_id,
        &profile.email,
        "success",
        &format!("{} [cycle:{}] {}", account_prefix, cycle_id, summary),
    );
    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "idle",
        "Idle - waiting for next sync cycle",
    );
    Ok(stats)
}

fn rebuild_sync_file_index(
    profile_id: &str,
    sync_state: &PersistedSyncState,
    local_snapshot: &HashMap<String, LocalSnapshotEntry>,
) -> Result<(), String> {
    reset_sync_file_index(profile_id)?;
    for remote_item in sync_state.remote_by_id.values() {
        upsert_remote_sync_file(profile_id, remote_item)?;
    }
    for (path, local_entry) in local_snapshot {
        upsert_local_sync_file(profile_id, path, local_entry)?;
    }
    Ok(())
}

fn new_cycle_id() -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let pid = std::process::id();
    format!("{}-{}", nanos, pid)
}

fn remaining_until_next_cycle(profile_id: &str, interval: Duration) -> Option<Duration> {
    let state = load_sync_state(profile_id).ok()?;
    let last_cycle_at = state.last_cycle_at?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(&last_cycle_at).ok()?;
    let elapsed = chrono::Utc::now().signed_duration_since(timestamp.with_timezone(&chrono::Utc));
    if elapsed.num_milliseconds() <= 0 {
        return Some(interval);
    }
    let elapsed_duration = Duration::from_millis(elapsed.num_milliseconds() as u64);
    if elapsed_duration >= interval {
        return None;
    }
    Some(interval - elapsed_duration)
}

fn load_syncable_profile(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<AccountProfile, String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profiles = load_profiles()?;
    let profile = profiles
        .into_iter()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    if profile.agent_state != "syncing" {
        return Err("Account is not in syncing state".to_string());
    }
    if !profile.auth_configured {
        return Err("Account is not authenticated".to_string());
    }
    Ok(profile)
}

fn update_profile_last_sync(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<(), String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    profile.last_sync_at = Some(chrono::Local::now().to_rfc3339());
    save_profiles(&profiles)
}
