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
    log::info!(
        "{} [cycle:{}] STAGE_START stage=scanning_local sync_root={}",
        account_prefix,
        cycle_id,
        sync_root.display()
    );
    let local_scan_estimated_total = if sync_state.local_snapshot.is_empty() {
        None
    } else {
        Some(sync_state.local_snapshot.len())
    };
    let local_snapshot = run_local_scan_with_runtime_updates(
        &graph.account_prefix,
        &graph.cycle_id,
        &graph.sync_runtime,
        profile_id,
        &sync_root,
        local_scan_estimated_total,
        cancel_flag,
    )
    .await?;
    log::info!(
        "{} [cycle:{}] STAGE_COMPLETE stage=scanning_local local_items={}",
        account_prefix,
        cycle_id,
        local_snapshot.len()
    );
    let index_started_at = std::time::Instant::now();
    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "building_index",
        "Building sync index",
    );
    log::info!(
        "{} [cycle:{}] STAGE_START stage=index_rebuild remote_items={} local_items={}",
        account_prefix,
        cycle_id,
        sync_state.remote_by_id.len(),
        local_snapshot.len()
    );
    rebuild_sync_file_index(
        &graph.sync_runtime,
        profile_id,
        &sync_state,
        &local_snapshot,
        &graph.account_prefix,
        &graph.cycle_id,
    )?;
    log::info!(
        "{} [cycle:{}] STAGE_COMPLETE stage=index_rebuild duration_ms={}",
        account_prefix,
        cycle_id,
        index_started_at.elapsed().as_millis()
    );
    let planner_started_at = std::time::Instant::now();
    runtime_set_phase(
        &graph.sync_runtime,
        profile_id,
        "planning_actions",
        "Planning sync actions",
    );
    log::info!(
        "{} [cycle:{}] STAGE_START stage=planner two_way_ready={}",
        account_prefix,
        cycle_id,
        sync_state.two_way_ready
    );
    let planner_counters = recompute_sync_file_actions(profile_id, sync_state.two_way_ready)?;
    log::info!(
        "{} [cycle:{}] STAGE_COMPLETE stage=planner duration_ms={} need_download={} need_upload={} conflicts={}",
        account_prefix,
        cycle_id,
        planner_started_at.elapsed().as_millis(),
        planner_counters.need_download_total,
        planner_counters.need_upload_total,
        planner_counters.conflict_total
    );
    runtime_set_upload_planned_total(
        &graph.sync_runtime,
        profile_id,
        planner_counters.need_upload_total,
    );
    log::info!(
        "{} [cycle:{}] SYNC_PLANNER_SUMMARY cloud_discovered={} local_discovered={} need_download={} need_upload={} conflicts={} shared_references_excluded={}",
        account_prefix,
        cycle_id,
        planner_counters.cloud_discovered_total,
        planner_counters.local_discovered_total,
        planner_counters.need_download_total,
        planner_counters.need_upload_total,
        planner_counters.conflict_total,
        planner_counters.shared_reference_total,
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
            && sync_state.bootstrap_full_scan_completed
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
            let blocked_message = if download_counters.failed_terminal > 0 {
                format!(
                    "Initial sync blocked: {} failed download{} need retry before two-way sync",
                    download_counters.failed_terminal,
                    if download_counters.failed_terminal == 1 { "" } else { "s" }
                )
            } else {
                "Initial sync in progress - downloading cloud files only".to_string()
            };
            runtime_set_phase(
                &graph.sync_runtime,
                profile_id,
                "syncing",
                &blocked_message,
            );
            if download_counters.failed_terminal > 0 {
                let failed_samples = list_terminal_failed_download_paths(profile_id, 5)
                    .unwrap_or_else(|_| Vec::new());
                log::warn!(
                    "{} [cycle:{}] TWO_WAY_BLOCK_REASON reason=failed_terminal_downloads count={} sample_paths={}",
                    account_prefix,
                    cycle_id,
                    download_counters.failed_terminal,
                    if failed_samples.is_empty() {
                        "(none)".to_string()
                    } else {
                        failed_samples.join(" | ")
                    }
                );
            }
            if planner_counters.need_download_total > 0 {
                log::warn!(
                    "{} [cycle:{}] TWO_WAY_BLOCK_REASON reason=planner_requires_downloads need_download={} queue_remaining={} failed_terminal={}",
                    account_prefix,
                    cycle_id,
                    planner_counters.need_download_total,
                    download_counters.remaining,
                    download_counters.failed_terminal
                );
            }
            log::warn!(
                "{} [cycle:{}] BOOTSTRAP_TWO_WAY_BLOCKED cursor_active={} bootstrap_full_scan_completed={} planner_need_download={} queue_remaining={} failed_terminal={}",
                account_prefix,
                cycle_id,
                sync_state.active_delta_next_link.is_some(),
                sync_state.bootstrap_full_scan_completed,
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

async fn run_local_scan_with_runtime_updates(
    account_prefix: &str,
    cycle_id: &str,
    sync_runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    sync_root: &Path,
    estimated_total: Option<usize>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<HashMap<String, LocalSnapshotEntry>, String> {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    let scanned_count = Arc::new(AtomicUsize::new(0));
    let last_progress_unix = Arc::new(AtomicI64::new(chrono::Utc::now().timestamp()));
    let current_path = Arc::new(Mutex::new(None::<String>));

    let thread_scanned_count = Arc::clone(&scanned_count);
    let thread_last_progress_unix = Arc::clone(&last_progress_unix);
    let thread_current_path = Arc::clone(&current_path);
    let sync_root_owned = sync_root.to_path_buf();

    let mut scan_handle = tauri::async_runtime::spawn_blocking(move || {
        collect_local_snapshot_with_progress(&sync_root_owned, |relative_path| {
            thread_scanned_count.fetch_add(1, AtomicOrdering::Relaxed);
            thread_last_progress_unix.store(chrono::Utc::now().timestamp(), AtomicOrdering::Relaxed);
            if let Ok(mut value) = thread_current_path.lock() {
                *value = Some(relative_path.to_string());
            }
        })
    });

    let mut last_emit_at = std::time::Instant::now();
    let mut last_progress_log_at = std::time::Instant::now();
    let scan_started_at = std::time::Instant::now();
    let mut last_stall_warn_at: Option<std::time::Instant> = None;
    log::info!(
        "{} [cycle:{}] LOCAL_SCAN_START sync_root={}",
        account_prefix,
        cycle_id,
        sync_root.display()
    );
    runtime_set_local_scan_progress(sync_runtime, profile_id, 0, estimated_total, None);

    loop {
        ensure_not_cancelled(cancel_flag)?;
        tokio::select! {
            scan_result = &mut scan_handle => {
                let local_snapshot = scan_result
                    .map_err(|error| format!("Local scan task failed: {error}"))??;
                log::info!(
                    "{} [cycle:{}] LOCAL_SCAN_COMPLETE scanned={} duration_ms={}",
                    account_prefix,
                    cycle_id,
                    local_snapshot.len(),
                    scan_started_at.elapsed().as_millis()
                );
                runtime_set_local_scan_progress(
                    sync_runtime,
                    profile_id,
                    local_snapshot.len(),
                    Some(local_snapshot.len()),
                    None,
                );
                return Ok(local_snapshot);
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                let now_unix = chrono::Utc::now().timestamp();
                let last_progress = last_progress_unix.load(AtomicOrdering::Relaxed);
                let stalled_seconds = now_unix.saturating_sub(last_progress);
                let scanned = scanned_count.load(AtomicOrdering::Relaxed);
                let path = current_path.lock().ok().and_then(|value| value.clone());

                if last_emit_at.elapsed() >= Duration::from_millis(350) {
                    runtime_set_local_scan_progress(
                        sync_runtime,
                        profile_id,
                        scanned,
                        estimated_total,
                        path.as_deref(),
                    );
                    last_emit_at = std::time::Instant::now();
                }

                if last_progress_log_at.elapsed() >= Duration::from_secs(10) {
                    log::info!(
                        "{} [cycle:{}] LOCAL_SCAN_PROGRESS scanned={} elapsed_s={} current_path={}",
                        account_prefix,
                        cycle_id,
                        scanned,
                        scan_started_at.elapsed().as_secs(),
                        path.as_deref().unwrap_or("(none)")
                    );
                    last_progress_log_at = std::time::Instant::now();
                }

                if stalled_seconds >= 60 {
                    let should_warn = last_stall_warn_at
                        .as_ref()
                        .is_none_or(|value| value.elapsed() >= Duration::from_secs(30));
                    if should_warn {
                        log::warn!(
                            "{} [cycle:{}] LOCAL_SCAN_STALLED stalled_for={}s scanned={} current_path={}",
                            account_prefix,
                            cycle_id,
                            stalled_seconds,
                            scanned,
                            path.as_deref().unwrap_or("(none)")
                        );
                        runtime_set_phase(
                            sync_runtime,
                            profile_id,
                            "scanning_local",
                            &format!(
                                "Local scan is taking longer than expected (scanned {} items)",
                                scanned
                            ),
                        );
                        last_stall_warn_at = Some(std::time::Instant::now());
                    }
                }
            }
        }
    }
}

fn rebuild_sync_file_index(
    sync_runtime: &Arc<std::sync::Mutex<SyncRuntimeMap>>,
    profile_id: &str,
    sync_state: &PersistedSyncState,
    local_snapshot: &HashMap<String, LocalSnapshotEntry>,
    account_prefix: &str,
    cycle_id: &str,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("Failed opening sync index transaction: {error}"))?;

    transaction
        .execute(
            "DELETE FROM sync_files WHERE profile_id = ?1",
            params![profile_id],
        )
        .map_err(|error| format!("Failed resetting sync file index: {error}"))?;

    let now = current_unix_seconds();
    let mut remote_statement = transaction
        .prepare(
            "INSERT INTO sync_files (
                profile_id, path, is_dir, is_shared_reference, shared_drive_id, shared_item_id, shared_kind, remote_item_id,
                remote_present, local_present,
                remote_size, local_size,
                remote_modified_ts, local_modified_ts,
                desired_action, conflict_state, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                1, 0,
                ?9, 0,
                ?10, 0,
                'none', NULL, ?11
             )
             ON CONFLICT(profile_id, path)
             DO UPDATE SET
                 is_dir = excluded.is_dir,
                 is_shared_reference = excluded.is_shared_reference,
                 shared_drive_id = excluded.shared_drive_id,
                 shared_item_id = excluded.shared_item_id,
                 shared_kind = excluded.shared_kind,
                 remote_item_id = excluded.remote_item_id,
                 remote_present = 1,
                 remote_size = excluded.remote_size,
                remote_modified_ts = excluded.remote_modified_ts,
                updated_at = excluded.updated_at",
        )
        .map_err(|error| format!("Failed preparing remote sync file upsert: {error}"))?;

    let mut local_statement = transaction
        .prepare(
            "INSERT INTO sync_files (
                profile_id, path, is_dir, is_shared_reference, shared_drive_id, shared_item_id, shared_kind, remote_item_id,
                remote_present, local_present,
                remote_size, local_size,
                remote_modified_ts, local_modified_ts,
                desired_action, conflict_state, updated_at
             ) VALUES (
                ?1, ?2, ?3, 0, NULL, NULL, NULL, NULL,
                0, 1,
                0, ?4,
                0, ?5,
                'none', NULL, ?6
             )
             ON CONFLICT(profile_id, path)
             DO UPDATE SET
                is_dir = excluded.is_dir,
                local_present = 1,
                local_size = excluded.local_size,
                local_modified_ts = excluded.local_modified_ts,
                updated_at = excluded.updated_at",
        )
        .map_err(|error| format!("Failed preparing local sync file upsert: {error}"))?;

    let total_entries = sync_state
        .remote_by_id
        .len()
        .saturating_add(local_snapshot.len());
    let mut processed_entries = 0_usize;
    let mut last_progress_emit_at = std::time::Instant::now();

    runtime_set_current_activity(
        sync_runtime,
        profile_id,
        "building_index",
        "determinate",
        Some(0),
        Some(total_entries),
        Some("entries"),
        Some("Indexing remote snapshot"),
    );

    for remote_item in sync_state.remote_by_id.values() {
        remote_statement
            .execute(params![
                profile_id,
                remote_item.path,
                if remote_item.is_dir { 1 } else { 0 },
                if remote_item.is_shared_reference { 1 } else { 0 },
                remote_item.shared_drive_id.as_deref(),
                remote_item.shared_item_id.as_deref(),
                remote_item.shared_kind.as_deref(),
                remote_item.id,
                remote_item.size as i64,
                remote_item.modified_ts,
                now,
            ])
            .map_err(|error| format!("Failed upserting remote sync file row: {error}"))?;
        processed_entries = processed_entries.saturating_add(1);
        if processed_entries % 250 == 0 || last_progress_emit_at.elapsed() >= Duration::from_millis(600) {
            runtime_set_current_activity(
                sync_runtime,
                profile_id,
                "building_index",
                "determinate",
                Some(processed_entries),
                Some(total_entries),
                Some("entries"),
                Some("Indexing remote snapshot"),
            );
            log::info!(
                "{} [cycle:{}] INDEX_PROGRESS phase=remote current={} total={}",
                account_prefix,
                cycle_id,
                processed_entries,
                total_entries
            );
            last_progress_emit_at = std::time::Instant::now();
        }
    }

    runtime_set_current_activity(
        sync_runtime,
        profile_id,
        "building_index",
        "determinate",
        Some(processed_entries),
        Some(total_entries),
        Some("entries"),
        Some("Indexing local snapshot"),
    );

    for (path, local_entry) in local_snapshot {
        local_statement
            .execute(params![
                profile_id,
                path,
                if local_entry.is_dir { 1 } else { 0 },
                local_entry.size as i64,
                local_entry.modified_ts,
                now,
            ])
            .map_err(|error| format!("Failed upserting local sync file row: {error}"))?;
        processed_entries = processed_entries.saturating_add(1);
        if processed_entries % 250 == 0 || last_progress_emit_at.elapsed() >= Duration::from_millis(600) {
            runtime_set_current_activity(
                sync_runtime,
                profile_id,
                "building_index",
                "determinate",
                Some(processed_entries),
                Some(total_entries),
                Some("entries"),
                Some("Indexing local snapshot"),
            );
            log::info!(
                "{} [cycle:{}] INDEX_PROGRESS phase=local current={} total={}",
                account_prefix,
                cycle_id,
                processed_entries,
                total_entries
            );
            last_progress_emit_at = std::time::Instant::now();
        }
    }

    drop(remote_statement);
    drop(local_statement);

    transaction
        .commit()
        .map_err(|error| format!("Failed committing sync index transaction: {error}"))?;

    runtime_set_current_activity(
        sync_runtime,
        profile_id,
        "building_index",
        "determinate",
        Some(processed_entries),
        Some(total_entries),
        Some("entries"),
        Some("Index rebuild complete"),
    );

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
    let now = chrono::Local::now().to_rfc3339();
    persist_sync_lifecycle_last_sync_at(profile_id, Some(&now))?;

    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    profile.last_sync_at = Some(now);
    save_profiles(&profiles)
}
