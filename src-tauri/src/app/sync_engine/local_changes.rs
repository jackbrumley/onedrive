async fn apply_local_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    current_local_snapshot: &HashMap<String, LocalSnapshotEntry>,
    remote_applied_paths: &std::collections::HashSet<String>,
    planned_upload_paths: &std::collections::HashSet<String>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
    const ACTIVITY_EMIT_INTERVAL: Duration = Duration::from_millis(500);

    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_local",
        "Applying local changes",
    );
    let mut local_paths: Vec<String> = current_local_snapshot.keys().cloned().collect();
    local_paths.sort_by_key(|path| path.matches('/').count());
    let total_local_paths = local_paths.len();
    let mut local_paths_seen: usize = 0;
    let mut last_heartbeat_at = std::time::Instant::now();
    let mut last_activity_emit_at = std::time::Instant::now();
    runtime_set_current_activity(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_local",
        "determinate",
        Some(0),
        Some(total_local_paths),
        Some("paths"),
        Some("Evaluating local changes"),
        Some(&graph.cycle_id),
    );
    for path in local_paths {
        ensure_not_cancelled(cancel_flag)?;
        local_paths_seen += 1;

        if last_activity_emit_at.elapsed() >= ACTIVITY_EMIT_INTERVAL || local_paths_seen % 200 == 0 {
            runtime_set_current_activity(
                &graph.sync_runtime,
                &graph.profile_id,
                "applying_local",
                "determinate",
                Some(local_paths_seen),
                Some(total_local_paths),
                Some("paths"),
                Some("Evaluating local changes"),
                Some(&graph.cycle_id),
            );
            last_activity_emit_at = std::time::Instant::now();
        }

        if last_heartbeat_at.elapsed() >= HEARTBEAT_INTERVAL {
            log::info!(
                "{} [cycle:{}] SYNC_HEARTBEAT phase=applying_local paths_seen={} uploaded={} upload_failures={} upload_cooldown_skips={}",
                graph.account_prefix,
                graph.cycle_id,
                local_paths_seen,
                stats.uploaded_files,
                stats.upload_failures,
                stats.upload_cooldown_skips
            );
            last_heartbeat_at = std::time::Instant::now();
        }

        if is_safe_backup_artifact(&path) {
            continue;
        }
        let Some(local_entry) = current_local_snapshot.get(&path) else {
            continue;
        };
        let remote_id = sync_state.remote_path_to_id.get(&path).cloned();

        if local_entry.is_dir {
            if remote_id.is_none() {
                log::info!(
                    "{} [cycle:{}] REMOTE_DIR_CREATE_START path={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    path
                );
                let created = create_remote_folder(graph, &path, cancel_flag).await?;
                let known = remote_known_item_from_drive_item(created, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.created_remote_folders += 1;
            }
            continue;
        }

        if !planned_upload_paths.contains(&path) {
            continue;
        }

        if remote_applied_paths.contains(&path) {
            log::info!(
                "{} [cycle:{}] LOCAL_SKIP_REMOTE_APPLIED path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            continue;
        }

        log::info!(
            "{} [cycle:{}] LOCAL_CHANGE path={} is_dir={} size={} modified_ts={}",
            graph.account_prefix,
            graph.cycle_id,
            path,
            local_entry.is_dir,
            local_entry.size,
            local_entry.modified_ts
        );

        if let Some(existing_id) = remote_id {
            let remote_modified = sync_state
                .remote_by_id
                .get(&existing_id)
                .map(|item| item.modified_ts)
                .unwrap_or(0);
            if local_entry.modified_ts > remote_modified {
                let now = current_unix_seconds();
                if let Some(remaining_seconds) =
                    upload_cooldown_remaining_seconds(sync_state, &path, now)
                {
                    stats.upload_cooldown_skips += 1;
                    runtime_set_phase(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        "applying_local",
                        &format!(
                            "Upload retry delayed for '{}' (retry in {})",
                            path,
                            format_retry_in_text(remaining_seconds)
                        ),
                    );
                    log::info!(
                        "{} [cycle:{}] LOCAL_UPLOAD_COOLDOWN_SKIP path={} retry_in={}s",
                        graph.account_prefix,
                        graph.cycle_id,
                        path,
                        remaining_seconds
                    );
                    continue;
                }
                log::info!(
                    "{} [cycle:{}] LOCAL_UPLOAD_EXISTING path={} remote_id={} local_ts={} remote_ts={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    path,
                    existing_id,
                    local_entry.modified_ts,
                    remote_modified
                );
                match upload_file_by_path(graph, sync_root, &path, cancel_flag).await {
                    Ok(uploaded) => {
                        let known = remote_known_item_from_drive_item(uploaded, &path)?;
                        upsert_remote_known_item(sync_state, known);
                        clear_upload_failure_cooldown(sync_state, &path);
                        stats.uploaded_files += 1;
                    }
                    Err(error) => {
                        let (failure_count, cooldown_seconds) =
                            record_upload_failure_cooldown(sync_state, &path, now);
                        stats.upload_failures += 1;
                        log::warn!(
                            "{} [cycle:{}] LOCAL_UPLOAD_FAILED path={} reason={} failures={} cooldown={}s",
                            graph.account_prefix,
                            graph.cycle_id,
                            path,
                            error,
                            failure_count,
                            cooldown_seconds
                        );
                    }
                }
            }
        } else {
            let now = current_unix_seconds();
            if let Some(remaining_seconds) = upload_cooldown_remaining_seconds(sync_state, &path, now)
            {
                stats.upload_cooldown_skips += 1;
                runtime_set_phase(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    "applying_local",
                    &format!(
                        "Upload retry delayed for '{}' (retry in {})",
                        path,
                        format_retry_in_text(remaining_seconds)
                    ),
                );
                log::info!(
                    "{} [cycle:{}] LOCAL_UPLOAD_COOLDOWN_SKIP path={} retry_in={}s",
                    graph.account_prefix,
                    graph.cycle_id,
                    path,
                    remaining_seconds
                );
                continue;
            }
            log::info!(
                "{} [cycle:{}] LOCAL_UPLOAD_NEW path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            match upload_file_by_path(graph, sync_root, &path, cancel_flag).await {
                Ok(uploaded) => {
                    let known = remote_known_item_from_drive_item(uploaded, &path)?;
                    upsert_remote_known_item(sync_state, known);
                    clear_upload_failure_cooldown(sync_state, &path);
                    stats.uploaded_files += 1;
                }
                Err(error) => {
                    let (failure_count, cooldown_seconds) =
                        record_upload_failure_cooldown(sync_state, &path, now);
                    stats.upload_failures += 1;
                    log::warn!(
                        "{} [cycle:{}] LOCAL_UPLOAD_FAILED path={} reason={} failures={} cooldown={}s",
                        graph.account_prefix,
                        graph.cycle_id,
                        path,
                        error,
                        failure_count,
                        cooldown_seconds
                    );
                }
            }
        }
    }

    let mut deleted_paths: Vec<String> = sync_state
        .local_snapshot
        .keys()
        .filter(|path| !current_local_snapshot.contains_key(*path))
        .cloned()
        .collect();

    if sync_state.large_delete_guard_approved && !sync_state.large_delete_pending_paths.is_empty() {
        deleted_paths = sync_state.large_delete_pending_paths.clone();
    }

    if !sync_state.large_delete_guard_approved && !sync_state.large_delete_pending_paths.is_empty() {
        let pending_count = sync_state.large_delete_pending_paths.len();
        let issue_message = format!(
            "Large deletion detected: {} items. Review before deleting from cloud.",
            pending_count
        );
        runtime_set_issue(
            &graph.sync_runtime,
            &graph.profile_id,
            "large_delete_guard",
            &issue_message,
            &[
                "confirm_large_delete",
                "keep_cloud_files",
                "open_sync_root",
                "retry_sync",
            ],
            None,
            None,
        );
        runtime_set_phase(
            &graph.sync_runtime,
            &graph.profile_id,
            "paused",
            "Large deletion detected - review required",
        );
        log::warn!(
            "{} [cycle:{}] LARGE_DELETE_GUARD_BLOCKING pending_count={}",
            graph.account_prefix,
            graph.cycle_id,
            pending_count
        );
        return Ok(());
    }

    let remote_deleted_paths: Vec<String> = deleted_paths
        .iter()
        .filter(|path| sync_state.remote_path_to_id.contains_key(path.as_str()))
        .cloned()
        .collect();

    let large_delete_guard_threshold = resolve_large_delete_guard_threshold();
    if remote_deleted_paths.len() >= large_delete_guard_threshold {
        if !sync_state.large_delete_guard_approved {
            sync_state.large_delete_pending_paths = remote_deleted_paths;
            let pending_count = sync_state.large_delete_pending_paths.len();
            let issue_message = format!(
                "Large deletion detected: {} items. Review before deleting from cloud.",
                pending_count
            );
            runtime_set_issue(
                &graph.sync_runtime,
                &graph.profile_id,
                "large_delete_guard",
                &issue_message,
                &[
                    "confirm_large_delete",
                    "keep_cloud_files",
                    "open_sync_root",
                    "retry_sync",
                ],
                None,
                None,
            );
            runtime_set_phase(
                &graph.sync_runtime,
                &graph.profile_id,
                "paused",
                "Large deletion detected - review required",
            );
            log::warn!(
                "{} [cycle:{}] LARGE_DELETE_GUARD_TRIGGERED count={} threshold={}",
                graph.account_prefix,
                graph.cycle_id,
                pending_count,
                large_delete_guard_threshold
            );
            return Ok(());
        }

        log::warn!(
            "{} [cycle:{}] LARGE_DELETE_GUARD_CONFIRMED count={} threshold={}",
            graph.account_prefix,
            graph.cycle_id,
            remote_deleted_paths.len(),
            large_delete_guard_threshold
        );
        sync_state.large_delete_guard_approved = false;
        sync_state.large_delete_pending_paths.clear();
        runtime_clear_issue(&graph.sync_runtime, &graph.profile_id);
    } else if sync_state.large_delete_guard_approved {
        sync_state.large_delete_guard_approved = false;
        sync_state.large_delete_pending_paths.clear();
        runtime_clear_issue(&graph.sync_runtime, &graph.profile_id);
    }

    deleted_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));
    let total_deleted_paths = deleted_paths.len();
    let mut deleted_paths_seen: usize = 0;

    runtime_set_current_activity(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_local",
        "determinate",
        Some(0),
        Some(total_deleted_paths),
        Some("paths"),
        Some("Reconciling remote deletions"),
        Some(&graph.cycle_id),
    );

    for deleted_path in deleted_paths {
        ensure_not_cancelled(cancel_flag)?;
        deleted_paths_seen += 1;

        if last_activity_emit_at.elapsed() >= ACTIVITY_EMIT_INTERVAL || deleted_paths_seen % 100 == 0 {
            runtime_set_current_activity(
                &graph.sync_runtime,
                &graph.profile_id,
                "applying_local",
                "determinate",
                Some(deleted_paths_seen),
                Some(total_deleted_paths),
                Some("paths"),
                Some("Reconciling remote deletions"),
                Some(&graph.cycle_id),
            );
            last_activity_emit_at = std::time::Instant::now();
        }
        if last_heartbeat_at.elapsed() >= HEARTBEAT_INTERVAL {
            log::info!(
                "{} [cycle:{}] SYNC_HEARTBEAT phase=applying_local delete_paths_seen={} remote_deleted={} local_deleted={}",
                graph.account_prefix,
                graph.cycle_id,
                deleted_paths_seen,
                stats.deleted_remote,
                stats.deleted_local
            );
            last_heartbeat_at = std::time::Instant::now();
        }
        if let Some(remote_id) = sync_state.remote_path_to_id.get(&deleted_path).cloned() {
            log::info!(
                "{} [cycle:{}] REMOTE_DELETE_START path={} remote_id={}",
                graph.account_prefix,
                graph.cycle_id,
                deleted_path,
                remote_id
            );
            delete_remote_item(graph, &remote_id, cancel_flag).await?;
            sync_state.remote_path_to_id.remove(&deleted_path);
            sync_state.remote_by_id.remove(&remote_id);
            log::info!(
                "{} [cycle:{}] REMOTE_DELETE_OK path={} remote_id={}",
                graph.account_prefix,
                graph.cycle_id,
                deleted_path,
                remote_id
            );
            stats.deleted_remote += 1;
        }
    }

    Ok(())
}

async fn reconcile_bootstrap_local_snapshot(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut remote_items: Vec<RemoteKnownItem> = sync_state.remote_by_id.values().cloned().collect();
    remote_items.sort_by_key(|item| item.path.matches('/').count());
    let total_remote_items = remote_items.len();
    let mut remote_items_seen: usize = 0;
    let mut last_activity_emit_at = std::time::Instant::now();

    runtime_set_current_activity(
        &graph.sync_runtime,
        &graph.profile_id,
        "preparing_two_way_baseline",
        "determinate",
        Some(0),
        Some(total_remote_items),
        Some("items"),
        Some("Preparing two-way sync baseline"),
        Some(&graph.cycle_id),
    );

    for remote_item in remote_items {
        ensure_not_cancelled(cancel_flag)?;
        remote_items_seen = remote_items_seen.saturating_add(1);
        if last_activity_emit_at.elapsed() >= Duration::from_millis(500) || remote_items_seen % 100 == 0 {
            runtime_set_current_activity(
                &graph.sync_runtime,
                &graph.profile_id,
                "preparing_two_way_baseline",
                "determinate",
                Some(remote_items_seen),
                Some(total_remote_items),
                Some("items"),
                Some("Preparing two-way sync baseline"),
                Some(&graph.cycle_id),
            );
            last_activity_emit_at = std::time::Instant::now();
        }
        if sync_state.local_snapshot.contains_key(&remote_item.path) {
            continue;
        }

        let local_abs = sync_root.join(path_to_local(&remote_item.path));
        if remote_item.is_dir {
            std::fs::create_dir_all(&local_abs).map_err(|error| {
                format!(
                    "Failed creating local directory '{}' during initial sync: {}",
                    local_abs.display(),
                    error
                )
            })?;
            continue;
        }

        log::info!(
            "{} [cycle:{}] INITIAL_SYNC_RESTORE path={} id={}",
            graph.account_prefix,
            graph.cycle_id,
            remote_item.path,
            remote_item.id
        );
        let outcome = download_remote_item_content(
            graph,
            None,
            &remote_item.id,
            &remote_item.path,
            Some(remote_item.size),
            &local_abs,
            cancel_flag,
        )
        .await?;

        match outcome {
            RemoteDownloadOutcome::Downloaded => {
                if let Some(local_entry) = read_local_entry(&local_abs)? {
                    sync_state.local_snapshot.insert(remote_item.path.clone(), local_entry);
                }
                stats.downloaded_files += 1;
            }
            RemoteDownloadOutcome::SkippedMissingRemote => {
                sync_state.remote_by_id.remove(&remote_item.id);
                sync_state.remote_path_to_id.remove(&remote_item.path);
                sync_state.local_snapshot.remove(&remote_item.path);
                stats.remote_items_skipped_missing += 1;
                log::warn!(
                    "{} [cycle:{}] INITIAL_SYNC_RESTORE_SKIPPED_MISSING path={} id={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    remote_item.path,
                    remote_item.id
                );
            }
        }
    }

    Ok(())
}

fn upsert_remote_known_item(sync_state: &mut PersistedSyncState, item: RemoteKnownItem) {
    sync_state
        .remote_path_to_id
        .insert(item.path.clone(), item.id.clone());
    sync_state.remote_by_id.insert(item.id.clone(), item);
}

fn has_local_changed(current: &LocalSnapshotEntry, previous: Option<&LocalSnapshotEntry>) -> bool {
    match previous {
        Some(entry) => entry != current,
        None => true,
    }
}

fn is_safe_backup_artifact(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| name.contains("-safeBackup-"))
}

fn current_unix_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

fn upload_cooldown_remaining_seconds(sync_state: &PersistedSyncState, path: &str, now: i64) -> Option<i64> {
    let retry_after = *sync_state.upload_retry_after_by_path.get(path)?;
    if retry_after <= now {
        return None;
    }
    Some(retry_after - now)
}

fn format_retry_in_text(remaining_seconds: i64) -> String {
    if remaining_seconds < 60 {
        return format!("{}s", remaining_seconds);
    }
    let minutes = remaining_seconds / 60;
    let seconds = remaining_seconds % 60;
    if minutes < 60 {
        return format!("{}m {}s", minutes, seconds);
    }
    let hours = minutes / 60;
    let rem_minutes = minutes % 60;
    format!("{}h {}m", hours, rem_minutes)
}

fn clear_upload_failure_cooldown(sync_state: &mut PersistedSyncState, path: &str) {
    sync_state.upload_failure_counts_by_path.remove(path);
    sync_state.upload_retry_after_by_path.remove(path);
}

fn record_upload_failure_cooldown(sync_state: &mut PersistedSyncState, path: &str, now: i64) -> (u32, i64) {
    let failure_count = sync_state
        .upload_failure_counts_by_path
        .entry(path.to_string())
        .and_modify(|value| *value = value.saturating_add(1))
        .or_insert(1);
    let exponent = failure_count.saturating_sub(1).min(10);
    let cooldown_seconds = 2_i64.saturating_pow(exponent).min(1800);
    let retry_after = now.saturating_add(cooldown_seconds);
    sync_state
        .upload_retry_after_by_path
        .insert(path.to_string(), retry_after);
    (*failure_count, cooldown_seconds)
}

fn remove_local_path(sync_root: &Path, relative_path: &str) -> Result<(), String> {
    let full_path = sync_root.join(path_to_local(relative_path));
    if !full_path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&full_path).map_err(|error| error.to_string())?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(&full_path).map_err(|error| {
            format!(
                "Failed removing directory '{}': {}",
                full_path.display(),
                error
            )
        })
    } else {
        std::fs::remove_file(&full_path)
            .map_err(|error| format!("Failed removing file '{}': {}", full_path.display(), error))
    }
}

fn create_safe_backup(local_path: &Path) -> Result<Option<PathBuf>, String> {
    if !local_path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(local_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Ok(None);
    }

    let parent = local_path
        .parent()
        .ok_or_else(|| "Local backup path has no parent".to_string())?;
    let file_name = local_path
        .file_name()
        .ok_or_else(|| "Local backup path has no filename".to_string())?
        .to_string_lossy()
        .to_string();

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut index = 1_u32;
    loop {
        let backup_name = format!("{}-safeBackup-{}-{:04}", file_name, stamp, index);
        let backup_path = parent.join(backup_name);
        if !backup_path.exists() {
            std::fs::copy(local_path, &backup_path).map_err(|error| {
                format!(
                    "Failed creating safe backup '{}' from '{}': {}",
                    backup_path.display(),
                    local_path.display(),
                    error
                )
            })?;
            return Ok(Some(backup_path));
        }
        index += 1;
    }
}

fn resolve_delta_item_path(item: &DeltaItem) -> Option<String> {
    let name = item.name.as_ref()?.trim();
    if name.is_empty() {
        return None;
    }
    let base = item
        .parent_reference
        .as_ref()
        .and_then(|reference| reference.path.as_deref())
        .map(extract_root_relative)
        .unwrap_or_default();
    let combined = if base.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base, name)
    };
    Some(normalize_relative_path(&combined))
}

fn extract_root_relative(parent_path: &str) -> String {
    let mut value = parent_path.trim().to_string();
    if let Some(rest) = value.strip_prefix("/drive/root:") {
        value = rest.to_string();
    }
    value.trim_start_matches('/').to_string()
}

fn normalize_relative_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_to_local(relative_path: &str) -> PathBuf {
    let mut output = PathBuf::new();
    for segment in relative_path.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

fn parse_rfc3339_seconds(value: Option<&str>) -> i64 {
    value
        .and_then(|input| chrono::DateTime::parse_from_rfc3339(input).ok())
        .map(|timestamp| timestamp.timestamp())
        .unwrap_or(0)
}

fn shared_drive_id_from_delta_item(item: &DeltaItem) -> Option<String> {
    item.remote_item
        .as_ref()
        .and_then(|remote_item| remote_item.get("parentReference"))
        .and_then(|parent_reference| parent_reference.get("driveId"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn shared_item_id_from_delta_item(item: &DeltaItem) -> Option<String> {
    item.remote_item
        .as_ref()
        .and_then(|remote_item| remote_item.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn shared_kind_from_delta_item(item: &DeltaItem) -> Option<String> {
    let remote_item = item.remote_item.as_ref()?;
    if remote_item.get("folder").is_some() {
        return Some("folder".to_string());
    }
    if remote_item.get("file").is_some() {
        return Some("file".to_string());
    }
    if remote_item.get("package").is_some() {
        return Some("package".to_string());
    }
    Some("unknown".to_string())
}
