async fn fetch_and_apply_delta_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    const PHASE_PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
    const PHASE_PROGRESS_ITEM_STEP: usize = 250;

    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "scanning_remote",
        "Fetching remote file list",
    );
    let mut current_url = sync_state
        .active_delta_next_link
        .clone()
        .or_else(|| sync_state.delta_link.clone())
        .unwrap_or_else(|| format!("{GRAPH_ROOT}/me/drive/root/delta"));
    let scan_started_at = std::time::Instant::now();
    let mut last_phase_update_at = scan_started_at;
    let mut last_phase_update_items: usize = 0;

    loop {
        ensure_not_cancelled(cancel_flag)?;
        log::info!(
            "{} [cycle:{}] DELTA_PAGE_REQUEST url={}",
            graph.account_prefix,
            graph.cycle_id,
            current_url
        );
        let response_text = graph_get_text(graph, &current_url, cancel_flag).await?;
        let response: DeltaResponse = serde_json::from_str(&response_text)
            .map_err(|error| format!("Failed to decode delta response: {error}"))?;

        stats.remote_pages += 1;
        stats.remote_items_received += response.value.len();
        let has_next_link = response.next_link.is_some();

        let should_update_phase = stats.remote_pages == 1
            || last_phase_update_at.elapsed() >= PHASE_PROGRESS_UPDATE_INTERVAL
            || stats
                .remote_items_received
                .saturating_sub(last_phase_update_items)
                >= PHASE_PROGRESS_ITEM_STEP
            || !has_next_link;
        if should_update_phase {
            let elapsed_seconds = scan_started_at.elapsed().as_secs_f64();
            let progress_message = if elapsed_seconds > 0.0 {
                format!(
                    "Fetching remote file list - {} items across {} pages ({:.0} items/s)",
                    stats.remote_items_received,
                    stats.remote_pages,
                    stats.remote_items_received as f64 / elapsed_seconds
                )
            } else {
                format!(
                    "Fetching remote file list - {} items across {} pages",
                    stats.remote_items_received,
                    stats.remote_pages
                )
            };
            runtime_set_phase(
                &graph.sync_runtime,
                &graph.profile_id,
                "scanning_remote",
                &progress_message,
            );
            last_phase_update_at = std::time::Instant::now();
            last_phase_update_items = stats.remote_items_received;
        }

        log::info!(
            "{} [cycle:{}] DELTA_PAGE_RECEIVED page={} items={}",
            graph.account_prefix,
            graph.cycle_id,
            stats.remote_pages,
            response.value.len()
        );
        apply_remote_changes(
            graph,
            sync_root,
            &response.value,
            sync_state,
            stats,
            cancel_flag,
        )
        .await?;

        if let Some(next_link) = response.next_link {
            sync_state.active_delta_next_link = Some(next_link.clone());
            save_sync_state(&graph.profile_id, sync_state)?;
            current_url = next_link;
            continue;
        }

        if let Some(delta_link) = response.delta_link {
            sync_state.delta_link = Some(delta_link);
        }
        sync_state.active_delta_next_link = None;
        save_sync_state(&graph.profile_id, sync_state)?;
        break;
    }

    Ok(())
}

async fn apply_remote_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    changes: &[DeltaItem],
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut pending_downloads: Vec<(String, String, PathBuf, RemoteKnownItem)> = Vec::new();

    for item in changes {
        ensure_not_cancelled(cancel_flag)?;
        if item.deleted.is_some() {
            if let Some(existing) = sync_state.remote_by_id.get(&item.id).cloned() {
                log::info!(
                    "{} [cycle:{}] REMOTE_DELETE_ITEM id={} path={} is_dir={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    item.id,
                    existing.path,
                    existing.is_dir
                );
                let local_abs = sync_root.join(path_to_local(&existing.path));
                let local_current = read_local_entry(&local_abs)?;
                let previous_local = sync_state.local_snapshot.get(&existing.path);
                let local_changed = local_current
                    .as_ref()
                    .map(|entry| has_local_changed(entry, previous_local))
                    .unwrap_or(false);

                if local_changed && !existing.is_dir {
                    log::info!(
                        "{} [cycle:{}] REMOTE_DELETE_LOCAL_CHANGED_UPLOAD path={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        existing.path
                    );
                    let uploaded =
                        upload_file_by_path(graph, sync_root, &existing.path, cancel_flag).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &existing.path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                sync_state.remote_by_id.remove(&item.id);
                sync_state.remote_path_to_id.remove(&existing.path);
                sync_state.local_snapshot.remove(&existing.path);
                remove_local_path(sync_root, &existing.path)?;
                log::info!(
                    "{} [cycle:{}] LOCAL_DELETE_OK path={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    existing.path
                );
                stats.deleted_local += 1;
            }
            continue;
        }

        let Some(path) = resolve_delta_item_path(item) else {
            log::warn!(
                "{} [cycle:{}] DELTA_ITEM_SKIPPED id={} reason=missing_path",
                graph.account_prefix,
                graph.cycle_id,
                item.id
            );
            continue;
        };

        let remote_entry = RemoteKnownItem {
            id: item.id.clone(),
            path: path.clone(),
            is_dir: item.folder.is_some(),
            size: item.size.unwrap_or(0),
            modified_ts: parse_rfc3339_seconds(item.last_modified_date_time.as_deref()),
        };

        let local_abs = sync_root.join(path_to_local(&path));
        if remote_entry.is_dir {
            log::info!(
                "{} [cycle:{}] LOCAL_DIR_ENSURE path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            std::fs::create_dir_all(&local_abs).map_err(|error| {
                format!(
                    "Failed creating local directory '{}': {}",
                    local_abs.display(),
                    error
                )
            })?;
        } else {
            let local_current = read_local_entry(&local_abs)?;
            let previous_local = sync_state.local_snapshot.get(&path);
            let local_changed = local_current
                .as_ref()
                .map(|entry| has_local_changed(entry, previous_local))
                .unwrap_or(false);

            if local_changed {
                let local_entry = local_current.expect("local_changed implies local entry exists");
                if local_entry.modified_ts > remote_entry.modified_ts {
                    log::info!(
                        "{} [cycle:{}] REMOTE_OLDER_UPLOAD_LOCAL path={} local_ts={} remote_ts={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        path,
                        local_entry.modified_ts,
                        remote_entry.modified_ts
                    );
                    let uploaded =
                        upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                if let Some(backup_path) = create_safe_backup(&local_abs)? {
                    log::info!(
                        "{} [cycle:{}] SAFE_BACKUP_CREATED source={} backup={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        local_abs.display(),
                        backup_path.display()
                    );
                    let conflict_backup_relative = relative_path_for_issue(sync_root, &backup_path);
                    if let Ok(mut runtime_map) = graph.sync_runtime.lock() {
                        sync_runtime::set_issue(
                            &mut runtime_map,
                            &graph.profile_id,
                            "conflict_detected",
                            "Conflict detected. A safe backup was created.",
                            &["open_conflict", "open_sync_root", "retry_sync"],
                            Some(&path),
                            conflict_backup_relative.as_deref(),
                        );
                    }
                }
            }

            pending_downloads.push((item.id.clone(), path, local_abs, remote_entry.clone()));
            continue;
        }

        upsert_remote_known_item(sync_state, remote_entry);
    }

    if !pending_downloads.is_empty() {
        let download_concurrency = resolve_download_concurrency();
        log::info!(
            "{} [cycle:{}] REMOTE_DOWNLOAD_BATCH_START queued={} concurrency={}",
            graph.account_prefix,
            graph.cycle_id,
            pending_downloads.len(),
            download_concurrency
        );

        let mut download_tasks = stream::iter(pending_downloads.into_iter().map(|download| {
            let cancel_state = Arc::clone(cancel_flag);
            let graph_context = graph.clone();
            async move {
                let (item_id, path, local_abs, remote_entry) = download;
                match download_remote_item_content(
                    &graph_context,
                    &item_id,
                    &path,
                    &local_abs,
                    &cancel_state,
                )
                .await
                {
                    Ok(outcome) => Ok((item_id, path, remote_entry, outcome)),
                    Err(error) => Err(format!(
                        "Remote download failed item_id={} path={}: {}",
                        item_id, path, error
                    )),
                }
            }
        }))
        .buffer_unordered(download_concurrency);

        let mut completed_count: usize = 0;
        while let Some(task_result) = download_tasks.next().await {
            let (_, _, remote_entry, outcome) = task_result?;
            match outcome {
                RemoteDownloadOutcome::Downloaded => {
                    upsert_remote_known_item(sync_state, remote_entry);
                    stats.downloaded_files += 1;
                    completed_count += 1;
                }
                RemoteDownloadOutcome::SkippedMissingRemote => {
                    sync_state.remote_by_id.remove(&remote_entry.id);
                    sync_state.remote_path_to_id.remove(&remote_entry.path);
                    sync_state.local_snapshot.remove(&remote_entry.path);
                    stats.remote_items_skipped_missing += 1;
                    log::warn!(
                        "{} [cycle:{}] REMOTE_ITEM_SKIP_MISSING path={} id={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        remote_entry.path,
                        remote_entry.id
                    );
                }
            }
        }

        log::info!(
            "{} [cycle:{}] REMOTE_DOWNLOAD_BATCH_COMPLETE completed={}",
            graph.account_prefix,
            graph.cycle_id,
            completed_count
        );
    }

    Ok(())
}
