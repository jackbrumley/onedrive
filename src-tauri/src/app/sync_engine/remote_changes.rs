async fn fetch_and_apply_delta_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<HashSet<String>, String> {
    const PHASE_PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
    const PHASE_PROGRESS_ITEM_STEP: usize = 250;
    const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

    let delta_page_queue_capacity = resolve_delta_page_queue_capacity();
    let download_queue_capacity = resolve_download_queue_capacity();
    let checkpoint_flush_step = resolve_checkpoint_flush_step();
    let download_concurrency = resolve_download_concurrency();

    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "scanning_remote",
        "Fetching remote files",
    );
    runtime_set_remote_scan_complete(&graph.sync_runtime, &graph.profile_id, false);
    runtime_set_remote_download_in_flight(&graph.sync_runtime, &graph.profile_id, 0);
    log::info!(
        "{} [cycle:{}] REMOTE_PIPELINE_START delta_queue_capacity={} download_queue_capacity={} download_concurrency={} checkpoint_flush_step={}",
        graph.account_prefix,
        graph.cycle_id,
        delta_page_queue_capacity,
        download_queue_capacity,
        download_concurrency,
        checkpoint_flush_step
    );

    let start_url = sync_state
        .active_delta_next_link
        .clone()
        .or_else(|| sync_state.delta_link.clone())
        .unwrap_or_else(|| format!("{GRAPH_ROOT}/me/drive/root/delta"));
    let scan_started_at = std::time::Instant::now();
    let mut last_phase_update_at = scan_started_at;
    let mut last_phase_update_items: usize = 0;
    let mut last_progress_at = scan_started_at;
    let stall_timeout = resolve_stall_timeout();
    let mut heartbeat_ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut watchdog_ticker = tokio::time::interval(Duration::from_secs(2));
    watchdog_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let (page_tx, mut page_rx) =
        mpsc::channel::<Result<DeltaPageWorkItem, String>>(delta_page_queue_capacity);
    let mut producer_graph = graph.clone();
    let producer_cancel = Arc::clone(cancel_flag);
    tauri::async_runtime::spawn(async move {
        let mut current_url = start_url;
        loop {
            if let Err(error) = ensure_not_cancelled(&producer_cancel) {
                let _ = page_tx.send(Err(error)).await;
                return;
            }

            log::info!(
                "{} [cycle:{}] DELTA_PAGE_REQUEST url={}",
                producer_graph.account_prefix,
                producer_graph.cycle_id,
                current_url
            );
            let response_text = match graph_get_text(&mut producer_graph, &current_url, &producer_cancel).await {
                Ok(text) => text,
                Err(error) => {
                    let _ = page_tx.send(Err(error)).await;
                    return;
                }
            };

            let response: DeltaResponse = match serde_json::from_str(&response_text)
                .map_err(|error| format!("Failed to decode delta response: {error}"))
            {
                Ok(value) => value,
                Err(error) => {
                    let _ = page_tx.send(Err(error)).await;
                    return;
                }
            };

            let next_link = response.next_link.clone();
            let payload = DeltaPageWorkItem {
                items: response.value,
                next_link: response.next_link,
                delta_link: response.delta_link,
            };

            if page_tx.send(Ok(payload)).await.is_err() {
                return;
            }

            if let Some(next) = next_link {
                current_url = next;
                continue;
            }

            return;
        }
    });

    let (download_tx_raw, download_rx) =
        mpsc::channel::<RemoteDownloadJob>(download_queue_capacity);
    let mut download_tx = Some(download_tx_raw);
    let (download_result_tx, mut download_result_rx) =
        mpsc::channel::<Result<RemoteDownloadResult, String>>(download_queue_capacity);
    let download_rx = Arc::new(tokio::sync::Mutex::new(download_rx));

    for _ in 0..download_concurrency {
        let worker_graph = graph.clone();
        let worker_cancel = Arc::clone(cancel_flag);
        let worker_download_rx = Arc::clone(&download_rx);
        let worker_result_tx = download_result_tx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let maybe_job = {
                    let mut receiver = worker_download_rx.lock().await;
                    receiver.recv().await
                };

                let Some(job) = maybe_job else {
                    return;
                };

                let result = match download_remote_item_content(
                    &worker_graph,
                    &job.item_id,
                    &job.path,
                    &job.local_abs,
                    &worker_cancel,
                )
                .await
                {
                    Ok(outcome) => Ok(RemoteDownloadResult {
                        remote_entry: job.remote_entry,
                        outcome,
                    }),
                    Err(error) => {
                        runtime_record_remote_download_failed(
                            &worker_graph.sync_runtime,
                            &worker_graph.profile_id,
                            &job.item_id,
                        );
                        Err(format!(
                            "Remote download failed item_id={} path={}: {}",
                            job.item_id, job.path, error
                        ))
                    }
                };

                if worker_result_tx.send(result).await.is_err() {
                    return;
                }
            }
        });
    }
    drop(download_result_tx);

    let mut pending_download_ids: HashSet<String> = HashSet::new();
    let mut staged_download_jobs: HashMap<String, RemoteDownloadJob> = HashMap::new();
    let mut pending_download_count: usize = 0;
    let mut download_results_since_flush: usize = 0;
    let mut producer_done = false;
    let mut deferred_delta_link: Option<String> = None;
    let mut remote_applied_paths: HashSet<String> = HashSet::new();

    loop {
        ensure_not_cancelled(cancel_flag)?;
        if producer_done && pending_download_count == 0 {
            break;
        }

        tokio::select! {
            _ = heartbeat_ticker.tick() => {
                log::info!(
                    "{} [cycle:{}] SYNC_HEARTBEAT phase=scanning_remote pages={} items={} pending_downloads={} staged_downloads={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    stats.remote_pages,
                    stats.remote_items_received,
                    pending_download_count,
                    staged_download_jobs.len()
                );
            }
            _ = watchdog_ticker.tick() => {
                if last_progress_at.elapsed() >= stall_timeout {
                    log::warn!(
                        "{} [cycle:{}] SYNC_STALLED phase=scanning_remote no_progress_for={}s pages={} items={} pending_downloads={} staged_downloads={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        stall_timeout.as_secs(),
                        stats.remote_pages,
                        stats.remote_items_received,
                        pending_download_count,
                        staged_download_jobs.len()
                    );
                    return Err(format!(
                        "Remote sync stalled after {}s with no progress",
                        stall_timeout.as_secs()
                    ));
                }
            }
            maybe_page = page_rx.recv(), if !producer_done => {
                let page_payload = match maybe_page {
                    Some(Ok(value)) => value,
                    Some(Err(error)) => return Err(error),
                    None => {
                        producer_done = true;
                        continue;
                    }
                };
                last_progress_at = std::time::Instant::now();

                stats.remote_pages += 1;
                stats.remote_items_received += page_payload.items.len();
                let has_next_link = page_payload.next_link.is_some();
                let should_update_phase = stats.remote_pages == 1
                    || last_phase_update_at.elapsed() >= PHASE_PROGRESS_UPDATE_INTERVAL
                    || stats
                        .remote_items_received
                        .saturating_sub(last_phase_update_items)
                        >= PHASE_PROGRESS_ITEM_STEP
                    || !has_next_link;
                if should_update_phase {
                    let elapsed_seconds = scan_started_at.elapsed().as_secs_f64();
                    let progress_message = format!(
                        "Fetching remote files"
                    );
                    runtime_set_phase(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        "scanning_remote",
                        &progress_message,
                    );
                    if elapsed_seconds > 0.0 {
                        log::info!(
                            "{} [cycle:{}] REMOTE_SCAN_PROGRESS pages={} items={} rate_items_per_sec={:.0}",
                            graph.account_prefix,
                            graph.cycle_id,
                            stats.remote_pages,
                            stats.remote_items_received,
                            stats.remote_items_received as f64 / elapsed_seconds
                        );
                    } else {
                        log::info!(
                            "{} [cycle:{}] REMOTE_SCAN_PROGRESS pages={} items={}",
                            graph.account_prefix,
                            graph.cycle_id,
                            stats.remote_pages,
                            stats.remote_items_received
                        );
                    }
                    last_phase_update_at = std::time::Instant::now();
                    last_phase_update_items = stats.remote_items_received;
                }
                runtime_set_remote_download_in_flight(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    pending_download_count,
                );

                log::info!(
                    "{} [cycle:{}] DELTA_PAGE_RECEIVED page={} items={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    stats.remote_pages,
                    page_payload.items.len()
                );

                let active_download_tx = download_tx
                    .as_ref()
                    .ok_or_else(|| "Remote download queue unavailable".to_string())?;
                process_remote_page_items(
                    graph,
                    sync_root,
                    sync_state,
                    stats,
                    cancel_flag,
                    page_payload.items,
                    active_download_tx,
                    &mut pending_download_ids,
                    &mut staged_download_jobs,
                    &mut pending_download_count,
                )
                .await?;
                runtime_set_remote_download_in_flight(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    pending_download_count,
                );

                if let Some(next_link) = page_payload.next_link {
                    sync_state.active_delta_next_link = Some(next_link);
                    save_sync_state(&graph.profile_id, sync_state)?;
                } else {
                    deferred_delta_link = page_payload.delta_link;
                    producer_done = true;
                }
            }
            maybe_download_result = download_result_rx.recv(), if pending_download_count > 0 => {
                let download_result = match maybe_download_result {
                    Some(value) => value?,
                    None => return Err("Download worker channel closed unexpectedly".to_string()),
                };
                last_progress_at = std::time::Instant::now();
                let completed_download_id = download_result.remote_entry.id.clone();
                apply_remote_download_result(
                    graph,
                    sync_state,
                    stats,
                    &mut pending_download_ids,
                    &mut remote_applied_paths,
                    download_result,
                );
                pending_download_count = pending_download_count.saturating_sub(1);
                download_results_since_flush += 1;

                if let Some(next_job) = staged_download_jobs.remove(&completed_download_id) {
                    let active_download_tx = download_tx
                        .as_ref()
                        .ok_or_else(|| "Remote download queue unavailable".to_string())?;
                    active_download_tx
                        .send(next_job)
                        .await
                        .map_err(|_| "Remote download queue closed unexpectedly".to_string())?;
                    pending_download_count += 1;
                }

                if download_results_since_flush >= checkpoint_flush_step {
                    save_sync_state(&graph.profile_id, sync_state)?;
                    download_results_since_flush = 0;
                }

                runtime_set_remote_download_in_flight(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    pending_download_count,
                );

                if producer_done && pending_download_count > 0 {
                    runtime_set_phase(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        "applying_remote",
                        &format!("Finishing remote downloads - {} items remaining", pending_download_count),
                    );
                }
            }
        }
    }

    let _ = download_tx.take();
    if let Some(delta_link) = deferred_delta_link {
        sync_state.delta_link = Some(delta_link);
    }
    sync_state.active_delta_next_link = None;
    save_sync_state(&graph.profile_id, sync_state)?;
    runtime_set_remote_download_in_flight(&graph.sync_runtime, &graph.profile_id, 0);
    runtime_set_remote_scan_complete(&graph.sync_runtime, &graph.profile_id, true);

    Ok(remote_applied_paths)
}

async fn process_remote_page_items(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
    items: Vec<DeltaItem>,
    download_tx: &mpsc::Sender<RemoteDownloadJob>,
    pending_download_ids: &mut HashSet<String>,
    staged_download_jobs: &mut HashMap<String, RemoteDownloadJob>,
    pending_download_count: &mut usize,
) -> Result<(), String> {
    for item in items {
        ensure_not_cancelled(cancel_flag)?;
        runtime_record_remote_discovered(&graph.sync_runtime, &graph.profile_id, &item.id);
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
                    let now = current_unix_seconds();
                    if let Some(remaining_seconds) =
                        upload_cooldown_remaining_seconds(sync_state, &existing.path, now)
                    {
                        stats.upload_cooldown_skips += 1;
                        runtime_set_phase(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            "applying_remote",
                            &format!(
                                "Upload retry delayed for '{}' (retry in {})",
                                existing.path,
                                format_retry_in_text(remaining_seconds)
                            ),
                        );
                        log::info!(
                            "{} [cycle:{}] REMOTE_DELETE_LOCAL_UPLOAD_COOLDOWN_SKIP path={} retry_in={}s",
                            graph.account_prefix,
                            graph.cycle_id,
                            existing.path,
                            remaining_seconds
                        );
                        continue;
                    }
                    log::info!(
                        "{} [cycle:{}] REMOTE_DELETE_LOCAL_CHANGED_UPLOAD path={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        existing.path
                    );
                    match upload_file_by_path(graph, sync_root, &existing.path, cancel_flag).await {
                        Ok(uploaded) => {
                            let known = remote_known_item_from_drive_item(uploaded, &existing.path)?;
                            upsert_remote_known_item(sync_state, known);
                            clear_upload_failure_cooldown(sync_state, &existing.path);
                            stats.uploaded_files += 1;
                        }
                        Err(error) => {
                            let (failure_count, cooldown_seconds) =
                                record_upload_failure_cooldown(sync_state, &existing.path, now);
                            stats.upload_failures += 1;
                            log::warn!(
                                "{} [cycle:{}] REMOTE_DELETE_LOCAL_UPLOAD_FAILED path={} reason={} failures={} cooldown={}s",
                                graph.account_prefix,
                                graph.cycle_id,
                                existing.path,
                                error,
                                failure_count,
                                cooldown_seconds
                            );
                        }
                    }
                    continue;
                }

                sync_state.remote_by_id.remove(&item.id);
                sync_state.remote_path_to_id.remove(&existing.path);
                sync_state.local_snapshot.remove(&existing.path);
                pending_download_ids.remove(&item.id);
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

        let Some(path) = resolve_delta_item_path(&item) else {
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
            upsert_remote_known_item(sync_state, remote_entry);
            continue;
        }

        let local_current = read_local_entry(&local_abs)?;
        let previous_local = sync_state.local_snapshot.get(&path);
        let local_changed = local_current
            .as_ref()
            .map(|entry| has_local_changed(entry, previous_local))
            .unwrap_or(false);

            if local_changed {
                let local_entry = local_current.expect("local_changed implies local entry exists");
                if local_entry.modified_ts > remote_entry.modified_ts {
                    let now = current_unix_seconds();
                    if let Some(remaining_seconds) =
                        upload_cooldown_remaining_seconds(sync_state, &path, now)
                    {
                        stats.upload_cooldown_skips += 1;
                        runtime_set_phase(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            "applying_remote",
                            &format!(
                                "Upload retry delayed for '{}' (retry in {})",
                                path,
                                format_retry_in_text(remaining_seconds)
                            ),
                        );
                        log::info!(
                            "{} [cycle:{}] REMOTE_OLDER_UPLOAD_COOLDOWN_SKIP path={} retry_in={}s",
                            graph.account_prefix,
                            graph.cycle_id,
                            path,
                            remaining_seconds
                        );
                        continue;
                    }
                    log::info!(
                        "{} [cycle:{}] REMOTE_OLDER_UPLOAD_LOCAL path={} local_ts={} remote_ts={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        path,
                        local_entry.modified_ts,
                        remote_entry.modified_ts
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
                                "{} [cycle:{}] REMOTE_OLDER_UPLOAD_FAILED path={} reason={} failures={} cooldown={}s",
                                graph.account_prefix,
                                graph.cycle_id,
                                path,
                                error,
                                failure_count,
                                cooldown_seconds
                            );
                        }
                    }
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

        let job = RemoteDownloadJob {
            item_id: item.id,
            path,
            local_abs,
            remote_entry,
        };
        if pending_download_ids.insert(job.item_id.clone()) {
            runtime_record_remote_download_planned(
                &graph.sync_runtime,
                &graph.profile_id,
                &job.item_id,
            );
            download_tx
                .send(job)
                .await
                .map_err(|_| "Remote download queue closed unexpectedly".to_string())?;
            *pending_download_count += 1;
        } else {
            staged_download_jobs.insert(job.item_id.clone(), job);
        }
    }

    Ok(())
}

fn apply_remote_download_result(
    graph: &GraphContext,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    pending_download_ids: &mut HashSet<String>,
    remote_applied_paths: &mut HashSet<String>,
    result: RemoteDownloadResult,
) {
    pending_download_ids.remove(&result.remote_entry.id);
    match result.outcome {
        RemoteDownloadOutcome::Downloaded => {
            runtime_record_remote_download_completed(
                &graph.sync_runtime,
                &graph.profile_id,
                &result.remote_entry.id,
            );
            remote_applied_paths.insert(result.remote_entry.path.clone());
            upsert_remote_known_item(sync_state, result.remote_entry);
            stats.downloaded_files += 1;
        }
        RemoteDownloadOutcome::SkippedMissingRemote => {
            sync_state.remote_by_id.remove(&result.remote_entry.id);
            sync_state
                .remote_path_to_id
                .remove(&result.remote_entry.path);
            sync_state.local_snapshot.remove(&result.remote_entry.path);
            stats.remote_items_skipped_missing += 1;
            log::warn!(
                "{} [cycle:{}] REMOTE_ITEM_SKIP_MISSING path={} id={}",
                graph.account_prefix,
                graph.cycle_id,
                result.remote_entry.path,
                result.remote_entry.id
            );
        }
    }
}
