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
    runtime_set_remote_download_counters(&graph.sync_runtime, &graph.profile_id, 0, 0, 0, 0, 0);
    log::info!(
        "{} [cycle:{}] REMOTE_PIPELINE_START delta_queue_capacity={} download_queue_capacity={} download_concurrency={} checkpoint_flush_step={}",
        graph.account_prefix,
        graph.cycle_id,
        delta_page_queue_capacity,
        download_queue_capacity,
        download_concurrency,
        checkpoint_flush_step
    );

    let bootstrap_requires_authoritative_scan =
        !sync_state.two_way_ready && !sync_state.bootstrap_full_scan_completed;
    let start_url = if bootstrap_requires_authoritative_scan {
        if let Some(active_next_link) = sync_state.active_delta_next_link.clone() {
            active_next_link
        } else {
            if !sync_state.bootstrap_scan_initialized {
                log::info!(
                    "{} [cycle:{}] BOOTSTRAP_SCAN_START mode=authoritative_root_delta",
                    graph.account_prefix,
                    graph.cycle_id
                );
            }
            sync_state.bootstrap_scan_initialized = true;
            format!("{GRAPH_ROOT}/me/drive/root/delta")
        }
    } else {
        sync_state
            .active_delta_next_link
            .clone()
            .or_else(|| sync_state.delta_link.clone())
            .unwrap_or_else(|| format!("{GRAPH_ROOT}/me/drive/root/delta"))
    };
    let scan_started_at = std::time::Instant::now();
    let mut last_phase_update_at = scan_started_at;
    let mut last_phase_update_items: usize = 0;
    let mut last_progress_at = scan_started_at;
    let stall_timeout = resolve_stall_timeout();
    let mut heartbeat_ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut watchdog_ticker = tokio::time::interval(Duration::from_secs(2));
    watchdog_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let cycle_cancel_flag = Arc::new(AtomicBool::new(false));
    let cycle_cancel_bridge_flag = Arc::clone(&cycle_cancel_flag);
    let global_cancel_flag = Arc::clone(cancel_flag);
    tauri::async_runtime::spawn(async move {
        while !cycle_cancel_bridge_flag.load(Ordering::Relaxed) {
            if global_cancel_flag.load(Ordering::Relaxed) {
                cycle_cancel_bridge_flag.store(true, Ordering::Relaxed);
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let producer_alive = Arc::new(AtomicBool::new(false));
    let active_download_workers = Arc::new(AtomicUsize::new(0));
    let mut last_page_progress_at: Option<std::time::Instant> = None;
    let mut last_download_result_at: Option<std::time::Instant> = None;
    let mut last_download_counter_signature: Option<(usize, usize, usize, usize, u64, u64, u64)> =
        None;

    let (page_tx, mut page_rx) =
        mpsc::channel::<Result<DeltaPageWorkItem, String>>(delta_page_queue_capacity);
    let mut producer_graph = graph.clone();
    let producer_cancel = Arc::clone(&cycle_cancel_flag);
    let producer_alive_flag = Arc::clone(&producer_alive);
    tauri::async_runtime::spawn(async move {
        producer_alive_flag.store(true, Ordering::Relaxed);
        log::info!(
            "{} [cycle:{}] DELTA_PRODUCER_STARTED",
            producer_graph.account_prefix,
            producer_graph.cycle_id
        );
        let mut current_url = start_url;
        loop {
            if let Err(error) = ensure_not_cancelled(&producer_cancel) {
                producer_alive_flag.store(false, Ordering::Relaxed);
                log::info!(
                    "{} [cycle:{}] DELTA_PRODUCER_STOP reason=cancelled",
                    producer_graph.account_prefix,
                    producer_graph.cycle_id
                );
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
                    producer_alive_flag.store(false, Ordering::Relaxed);
                    log::warn!(
                        "{} [cycle:{}] DELTA_PRODUCER_STOP reason=graph_get_error error={}",
                        producer_graph.account_prefix,
                        producer_graph.cycle_id,
                        error
                    );
                    let _ = page_tx.send(Err(error)).await;
                    return;
                }
            };

            let response: DeltaResponse = match serde_json::from_str(&response_text)
                .map_err(|error| format!("Failed to decode delta response: {error}"))
            {
                Ok(value) => value,
                Err(error) => {
                    producer_alive_flag.store(false, Ordering::Relaxed);
                    log::warn!(
                        "{} [cycle:{}] DELTA_PRODUCER_STOP reason=decode_error error={}",
                        producer_graph.account_prefix,
                        producer_graph.cycle_id,
                        error
                    );
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
                producer_alive_flag.store(false, Ordering::Relaxed);
                log::info!(
                    "{} [cycle:{}] DELTA_PRODUCER_STOP reason=page_channel_closed",
                    producer_graph.account_prefix,
                    producer_graph.cycle_id
                );
                return;
            }

            if let Some(next) = next_link {
                current_url = next;
                continue;
            }

            producer_alive_flag.store(false, Ordering::Relaxed);
            log::info!(
                "{} [cycle:{}] DELTA_PRODUCER_STOP reason=scan_complete",
                producer_graph.account_prefix,
                producer_graph.cycle_id
            );
            return;
        }
    });

    let (download_tx_raw, download_rx) = mpsc::channel::<RemoteDownloadJob>(download_queue_capacity);
    let mut download_tx = Some(download_tx_raw);
    let (download_result_tx, mut download_result_rx) =
        mpsc::channel::<RemoteDownloadResult>(download_queue_capacity);
    let download_rx = Arc::new(tokio::sync::Mutex::new(download_rx));
    let pending_download_count = Arc::new(AtomicUsize::new(0));

    for worker_index in 0..download_concurrency {
        let worker_graph = graph.clone();
        let worker_cancel = Arc::clone(&cycle_cancel_flag);
        let worker_download_rx = Arc::clone(&download_rx);
        let worker_result_tx = download_result_tx.clone();
        let active_worker_count = Arc::clone(&active_download_workers);
        let worker_pending_download_count = Arc::clone(&pending_download_count);
        tauri::async_runtime::spawn(async move {
            active_worker_count.fetch_add(1, Ordering::Relaxed);
            log::info!(
                "{} [cycle:{}] DOWNLOAD_WORKER_STARTED worker_index={} active_workers={}",
                worker_graph.account_prefix,
                worker_graph.cycle_id,
                worker_index,
                active_worker_count.load(Ordering::Relaxed)
            );
            loop {
                let maybe_job = {
                    let mut receiver = worker_download_rx.lock().await;
                    receiver.recv().await
                };

                let Some(job) = maybe_job else {
                    log::info!(
                        "{} [cycle:{}] DOWNLOAD_WORKER_STOP worker_index={} reason=queue_closed",
                        worker_graph.account_prefix,
                        worker_graph.cycle_id,
                        worker_index
                    );
                    break;
                };

                let result = match download_remote_item_content(
                    &worker_graph,
                    Some(job.job_id),
                    &job.item_id,
                    &job.path,
                    Some(job.remote_entry.size),
                    &job.local_abs,
                    &worker_cancel,
                )
                .await
                {
                    Ok(outcome) => RemoteDownloadResult {
                        job_id: job.job_id,
                        remote_entry: job.remote_entry,
                        status: RemoteDownloadResultStatus::Success(outcome),
                    },
                    Err(error) => {
                        if error == DOWNLOAD_RETRY_DEFERRED_ERROR {
                            RemoteDownloadResult {
                                job_id: job.job_id,
                                remote_entry: job.remote_entry,
                                status: RemoteDownloadResultStatus::DeferredRetry,
                            }
                        } else if is_sync_cancelled_error(&error) {
                            RemoteDownloadResult {
                                job_id: job.job_id,
                                remote_entry: job.remote_entry,
                                status: RemoteDownloadResultStatus::Cancelled,
                            }
                        } else {
                            RemoteDownloadResult {
                                job_id: job.job_id,
                                remote_entry: job.remote_entry,
                                status: RemoteDownloadResultStatus::Failed(format!(
                                    "Remote download failed item_id={} path={}: {}",
                                    job.item_id, job.path, error
                                )),
                            }
                        }
                    }
                };
                let _ = worker_pending_download_count.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |current| Some(current.saturating_sub(1)),
                );

                if let Err(send_error) = worker_result_tx.send(result).await {
                    let dropped_result = send_error.0;
                    if let Err(error) = mark_download_job_retry_wait(
                        &worker_graph.profile_id,
                        dropped_result.job_id,
                        "Download worker result channel closed; retry scheduled",
                        Duration::from_secs(1),
                    ) {
                        log::warn!(
                            "{} [cycle:{}] DOWNLOAD_WORKER_RESULT_REQUEUE_FAILED worker_index={} job_id={} error={}",
                            worker_graph.account_prefix,
                            worker_graph.cycle_id,
                            worker_index,
                            dropped_result.job_id,
                            error
                        );
                    }
                    log::info!(
                        "{} [cycle:{}] DOWNLOAD_WORKER_STOP worker_index={} reason=result_channel_closed",
                        worker_graph.account_prefix,
                        worker_graph.cycle_id,
                        worker_index
                    );
                    break;
                }
            }
            let remaining = active_worker_count
                .fetch_sub(1, Ordering::Relaxed)
                .saturating_sub(1);
            log::info!(
                "{} [cycle:{}] DOWNLOAD_WORKER_EXITED worker_index={} active_workers={}",
                worker_graph.account_prefix,
                worker_graph.cycle_id,
                worker_index,
                remaining
            );
        });
    }
    drop(download_result_tx);

    let dispatcher_stop = Arc::new(AtomicBool::new(false));
    let dispatcher_graph = graph.clone();
    let dispatcher_sync_root = sync_root.to_path_buf();
    let dispatcher_pending_download_count = Arc::clone(&pending_download_count);
    let dispatcher_stop_flag = Arc::clone(&dispatcher_stop);
    let dispatcher_download_tx = download_tx
        .as_ref()
        .ok_or_else(|| "Remote download queue unavailable".to_string())?
        .clone();
    tauri::async_runtime::spawn(async move {
        let mut dispatch_ticker = tokio::time::interval(Duration::from_millis(50));
        dispatch_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            dispatch_ticker.tick().await;
            if dispatcher_stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let pending = dispatcher_pending_download_count.load(Ordering::Relaxed);
            let available_slots = download_concurrency.saturating_sub(pending);
            if available_slots == 0 {
                continue;
            }

            let counters = match read_download_job_counters(&dispatcher_graph.profile_id) {
                Ok(value) => value,
                Err(error) => {
                    log::warn!(
                        "{} [cycle:{}] DOWNLOAD_DISPATCHER_COUNTER_READ_FAILED error={}",
                        dispatcher_graph.account_prefix,
                        dispatcher_graph.cycle_id,
                        error
                    );
                    continue;
                }
            };
            if counters.remaining == 0 {
                continue;
            }

            match dispatch_claimed_download_jobs(
                &dispatcher_graph,
                &dispatcher_sync_root,
                &dispatcher_download_tx,
                available_slots,
                pending,
                counters.remaining,
            )
            .await
            {
                Ok(dispatched) => {
                    if dispatched > 0 {
                        dispatcher_pending_download_count
                            .fetch_add(dispatched, Ordering::Relaxed);
                    }
                }
                Err(error) => {
                    if error.contains("queue closed unexpectedly") {
                        log::info!(
                            "{} [cycle:{}] DOWNLOAD_DISPATCHER_STOP reason=queue_closed",
                            dispatcher_graph.account_prefix,
                            dispatcher_graph.cycle_id
                        );
                        break;
                    }
                    log::warn!(
                        "{} [cycle:{}] DOWNLOAD_DISPATCHER_FAILED error={}",
                        dispatcher_graph.account_prefix,
                        dispatcher_graph.cycle_id,
                        error
                    );
                }
            }
        }
    });

    let mut download_results_since_flush: usize = 0;
    let mut producer_done = false;
    let mut deferred_delta_link: Option<String> = None;
    let mut remote_applied_paths: HashSet<String> = HashSet::new();
    let mut terminal_error: Option<String> = None;

    loop {
        if let Err(error) = ensure_not_cancelled(&cycle_cancel_flag) {
            terminal_error = Some(error);
            break;
        }
        let download_counters = sync_download_counters_from_db(graph)?;
        let current_counter_signature = (
            download_counters.remaining,
            download_counters.in_progress,
            download_counters.retry_waiting,
            download_counters.completed,
            download_counters.completed_bytes,
            download_counters.in_flight_bytes_done,
            download_counters.remaining_bytes,
        );
        if last_download_counter_signature != Some(current_counter_signature) {
            let now = std::time::Instant::now();
            last_progress_at = now;
            last_download_result_at = Some(now);
            last_download_counter_signature = Some(current_counter_signature);
        }
        let current_pending_downloads = pending_download_count.load(Ordering::Relaxed);
        if producer_done && current_pending_downloads == 0 && download_counters.remaining == 0 {
            break;
        }

        tokio::select! {
            biased;
            maybe_download_result = download_result_rx.recv() => {
                let download_result = match maybe_download_result {
                    Some(value) => value,
                    None => {
                        if producer_done && pending_download_count.load(Ordering::Relaxed) == 0 {
                            break;
                        }
                        log::warn!(
                            "{} [cycle:{}] REMOTE_PIPELINE_ABORT reason=download_result_channel_closed producer_alive={} active_workers={} pending_downloads={} remaining_download_jobs={} retry_waiting={}",
                            graph.account_prefix,
                            graph.cycle_id,
                            producer_alive.load(Ordering::Relaxed),
                            active_download_workers.load(Ordering::Relaxed),
                            pending_download_count.load(Ordering::Relaxed),
                            download_counters.remaining,
                            download_counters.retry_waiting
                        );
                        terminal_error = Some("Download worker channel closed unexpectedly".to_string());
                        break;
                    },
                };
                last_progress_at = std::time::Instant::now();
                last_download_result_at = Some(last_progress_at);
                apply_remote_download_result(
                    graph,
                    sync_state,
                    stats,
                    &mut remote_applied_paths,
                    download_result,
                )?;
                download_results_since_flush += 1;

                if download_results_since_flush >= checkpoint_flush_step {
                    save_sync_state(&graph.profile_id, sync_state)?;
                    download_results_since_flush = 0;
                }

                let latest_counters = sync_download_counters_from_db(graph)?;
                if producer_done && latest_counters.remaining > 0 {
                    runtime_set_phase(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        "applying_remote",
                        &format!("Finishing remote downloads - {} items remaining", latest_counters.remaining),
                    );
                }
            }
            _ = heartbeat_ticker.tick() => {
                log::info!(
                    "{} [cycle:{}] SYNC_HEARTBEAT phase=scanning_remote pages={} items={} pending_downloads={} remaining_download_jobs={} retry_waiting={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    stats.remote_pages,
                    stats.remote_items_received,
                    pending_download_count.load(Ordering::Relaxed),
                    download_counters.remaining,
                    download_counters.retry_waiting
                );
            }
            _ = watchdog_ticker.tick() => {
                if last_progress_at.elapsed() >= stall_timeout {
                    let last_page_progress = last_page_progress_at
                        .map(|instant| format!("{}s", instant.elapsed().as_secs()))
                        .unwrap_or_else(|| "none".to_string());
                    let last_download_progress = last_download_result_at
                        .map(|instant| format!("{}s", instant.elapsed().as_secs()))
                        .unwrap_or_else(|| "none".to_string());
                    log::warn!(
                        "{} [cycle:{}] SYNC_STALLED phase=scanning_remote no_progress_for={}s pages={} items={} pending_downloads={} remaining_download_jobs={} in_progress={} retry_waiting={} producer_alive={} active_workers={} last_page_progress={} last_download_result_progress={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        stall_timeout.as_secs(),
                        stats.remote_pages,
                        stats.remote_items_received,
                        pending_download_count.load(Ordering::Relaxed),
                        download_counters.remaining,
                        download_counters.in_progress,
                        download_counters.retry_waiting,
                        producer_alive.load(Ordering::Relaxed),
                        active_download_workers.load(Ordering::Relaxed),
                        last_page_progress,
                        last_download_progress
                    );
                    if download_counters.in_progress > 0 {
                        last_progress_at = std::time::Instant::now();
                        continue;
                    }
                    if producer_done && download_counters.retry_waiting > 0 {
                        last_progress_at = std::time::Instant::now();
                        continue;
                    }
                    terminal_error = Some(format!(
                        "Remote sync stalled after {}s with no progress",
                        stall_timeout.as_secs()
                    ));
                    break;
                }
            }
            maybe_page = page_rx.recv(), if !producer_done => {
                let page_payload = match maybe_page {
                    Some(Ok(value)) => value,
                    Some(Err(error)) => {
                        log::warn!(
                            "{} [cycle:{}] REMOTE_PIPELINE_ABORT reason=producer_error error={} producer_alive={} active_workers={} pending_downloads={} remaining_download_jobs={} retry_waiting={}",
                            graph.account_prefix,
                            graph.cycle_id,
                            error,
                            producer_alive.load(Ordering::Relaxed),
                            active_download_workers.load(Ordering::Relaxed),
                            pending_download_count.load(Ordering::Relaxed),
                            download_counters.remaining,
                            download_counters.retry_waiting
                        );
                        terminal_error = Some(error);
                        break;
                    },
                    None => {
                        producer_done = true;
                        continue;
                    }
                };
                last_progress_at = std::time::Instant::now();
                last_page_progress_at = Some(last_progress_at);

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
                log::info!(
                    "{} [cycle:{}] DELTA_PAGE_RECEIVED page={} items={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    stats.remote_pages,
                    page_payload.items.len()
                );

                let process_result = process_remote_page_items(
                    graph,
                    sync_root,
                    sync_state,
                    stats,
                    &cycle_cancel_flag,
                    page_payload.items,
                )
                .await;
                if let Err(error) = process_result {
                    terminal_error = Some(error);
                    break;
                }
                let _ = sync_download_counters_from_db(graph)?;

                if let Some(next_link) = page_payload.next_link {
                    sync_state.active_delta_next_link = Some(next_link);
                    save_sync_state(&graph.profile_id, sync_state)?;
                } else {
                    deferred_delta_link = page_payload.delta_link;
                    producer_done = true;
                }
            }
        }
    }

    cycle_cancel_flag.store(true, Ordering::Relaxed);
    dispatcher_stop.store(true, Ordering::Relaxed);
    let _ = download_tx.take();
    if let Some(error) = terminal_error {
        return Err(error);
    }
    log::info!(
        "{} [cycle:{}] REMOTE_PIPELINE_DRAIN producer_alive={} active_workers={} pending_downloads={} remaining_download_jobs={}",
        graph.account_prefix,
        graph.cycle_id,
        producer_alive.load(Ordering::Relaxed),
        active_download_workers.load(Ordering::Relaxed),
        pending_download_count.load(Ordering::Relaxed),
        sync_download_counters_from_db(graph)?.remaining
    );
    if let Some(delta_link) = deferred_delta_link {
        sync_state.delta_link = Some(delta_link);
    }
    sync_state.active_delta_next_link = None;
    if !sync_state.two_way_ready {
        sync_state.bootstrap_full_scan_completed = true;
        sync_state.bootstrap_scan_initialized = true;
    }
    save_sync_state(&graph.profile_id, sync_state)?;
    let _ = sync_download_counters_from_db(graph)?;
    runtime_set_remote_scan_complete(&graph.sync_runtime, &graph.profile_id, true);
    log::info!(
        "{} [cycle:{}] REMOTE_PIPELINE_COMPLETE discovered={} downloaded={} skipped_missing={} producer_alive={} active_workers={}",
        graph.account_prefix,
        graph.cycle_id,
        stats.remote_items_received,
        stats.downloaded_files,
        stats.remote_items_skipped_missing,
        producer_alive.load(Ordering::Relaxed),
        active_download_workers.load(Ordering::Relaxed)
    );

    Ok(remote_applied_paths)
}

fn sync_download_counters_from_db(graph: &GraphContext) -> Result<DownloadJobCounters, String> {
    let counters = read_download_job_counters(&graph.profile_id)?;
    runtime_set_remote_download_counters(
        &graph.sync_runtime,
        &graph.profile_id,
        counters.planned_total,
        counters.completed,
        counters.failed_terminal,
        counters.in_progress,
        counters.retry_waiting,
    );
    Ok(counters)
}

async fn dispatch_claimed_download_jobs(
    graph: &GraphContext,
    sync_root: &Path,
    download_tx: &mpsc::Sender<RemoteDownloadJob>,
    available_slots: usize,
    pending_download_count: usize,
    remaining_download_jobs: usize,
) -> Result<usize, String> {
    if available_slots == 0 {
        return Ok(0);
    }

    let claimed = claim_download_jobs(&graph.profile_id, &graph.cycle_id, available_slots)?;
    if claimed.is_empty() {
        return Ok(0);
    }

    let mut dispatched: usize = 0;
    for claimed_job in claimed {
        let remote_entry = RemoteKnownItem {
            id: claimed_job.item_id.clone(),
            path: claimed_job.path.clone(),
            is_dir: false,
            size: claimed_job.remote_size,
            modified_ts: claimed_job.remote_modified_ts,
        };
        let local_abs = sync_root.join(path_to_local(&claimed_job.path));
        let job = RemoteDownloadJob {
            job_id: claimed_job.job_id,
            item_id: claimed_job.item_id,
            path: claimed_job.path,
            local_abs,
            remote_entry,
        };

        send_download_job_with_logging(
            graph,
            download_tx,
            job,
            pending_download_count + dispatched,
            remaining_download_jobs,
        )
        .await?;
        dispatched += 1;
    }
    Ok(dispatched)
}

async fn process_remote_page_items(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
    items: Vec<DeltaItem>,
) -> Result<(), String> {
    let bootstrap_cloud_first = !sync_state.two_way_ready;
    for item in items {
        ensure_not_cancelled(cancel_flag)?;
        runtime_record_remote_discovered(&graph.sync_runtime, &graph.profile_id, &item.id);
        if item.deleted.is_some() {
            let _ = remove_download_job_by_item_id(&graph.profile_id, &item.id);
            if bootstrap_cloud_first {
                if let Some(existing) = sync_state.remote_by_id.get(&item.id).cloned() {
                    sync_state.remote_by_id.remove(&item.id);
                    sync_state.remote_path_to_id.remove(&existing.path);
                    sync_state.local_snapshot.remove(&existing.path);
                }
                continue;
            }
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
            if !bootstrap_cloud_first {
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
            }
            upsert_remote_known_item(sync_state, remote_entry);
            continue;
        }

        if bootstrap_cloud_first {
            upsert_remote_known_item(sync_state, remote_entry.clone());
            let inserted = upsert_download_job(
                &graph.profile_id,
                &item.id,
                &path,
                remote_entry.size,
                remote_entry.modified_ts,
            )?;
            if inserted {
                runtime_record_remote_download_planned(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    &item.id,
                );
            }
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
                runtime_set_issue(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    "conflict_detected",
                    "Conflict detected. A safe backup was created.",
                    &["open_conflict", "open_sync_root", "retry_sync"],
                    Some(&path),
                    conflict_backup_relative.as_deref(),
                );
            }
        }

        let inserted = upsert_download_job(
            &graph.profile_id,
            &item.id,
            &path,
            remote_entry.size,
            remote_entry.modified_ts,
        )?;
        if inserted {
            runtime_record_remote_download_planned(
                &graph.sync_runtime,
                &graph.profile_id,
                &item.id,
            );
        }
    }

    Ok(())
}

async fn send_download_job_with_logging(
    graph: &GraphContext,
    download_tx: &mpsc::Sender<RemoteDownloadJob>,
    job: RemoteDownloadJob,
    pending_download_count: usize,
    remaining_download_jobs: usize,
) -> Result<(), String> {
    let item_id = job.item_id.clone();
    let path = job.path.clone();
    let enqueue_started_at = std::time::Instant::now();
    let mut wait_logged = false;
    let send_future = download_tx.send(job);
    tokio::pin!(send_future);
    loop {
        tokio::select! {
            send_result = &mut send_future => {
                if wait_logged {
                    log::info!(
                        "{} [cycle:{}] DOWNLOAD_ENQUEUE_WAIT_DONE item_id={} path={} wait_s={} pending_downloads={} remaining_download_jobs={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        item_id,
                        path,
                        enqueue_started_at.elapsed().as_secs(),
                        pending_download_count,
                        remaining_download_jobs
                    );
                }
                return send_result.map_err(|_| "Remote download queue closed unexpectedly".to_string());
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                wait_logged = true;
                log::warn!(
                    "{} [cycle:{}] DOWNLOAD_ENQUEUE_WAITING item_id={} path={} wait_s={} pending_downloads={} remaining_download_jobs={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    item_id,
                    path,
                    enqueue_started_at.elapsed().as_secs(),
                    pending_download_count,
                    remaining_download_jobs
                );
            }
        }
    }
}

fn apply_remote_download_result(
    graph: &GraphContext,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    remote_applied_paths: &mut HashSet<String>,
    result: RemoteDownloadResult,
) -> Result<(), String> {
    match result.status {
        RemoteDownloadResultStatus::DeferredRetry => {
            log::info!(
                "{} [cycle:{}] REMOTE_DOWNLOAD_JOB_RETRY_DEFERRED path={} id={}",
                graph.account_prefix,
                graph.cycle_id,
                result.remote_entry.path,
                result.remote_entry.id
            );
        }
        RemoteDownloadResultStatus::Cancelled => {
            let retry_reason = "Download cancelled due to pause; retry scheduled on resume";
            mark_download_job_retry_wait(
                &graph.profile_id,
                result.job_id,
                retry_reason,
                Duration::from_secs(1),
            )?;
            log::info!(
                "{} [cycle:{}] REMOTE_DOWNLOAD_JOB_CANCELLED_RETRY_WAIT path={} id={}",
                graph.account_prefix,
                graph.cycle_id,
                result.remote_entry.path,
                result.remote_entry.id
            );
        }
        RemoteDownloadResultStatus::Failed(error_text) => {
            mark_download_job_failed(&graph.profile_id, result.job_id, &error_text)?;
            runtime_record_remote_download_failed(
                &graph.sync_runtime,
                &graph.profile_id,
                &result.remote_entry.id,
            );
            log::warn!(
                "{} [cycle:{}] REMOTE_DOWNLOAD_JOB_FAILED path={} id={} error={}",
                graph.account_prefix,
                graph.cycle_id,
                result.remote_entry.path,
                result.remote_entry.id,
                error_text
            );
        }
        RemoteDownloadResultStatus::Success(outcome) => {
            let skipped = matches!(outcome, RemoteDownloadOutcome::SkippedMissingRemote);
            mark_download_job_done(&graph.profile_id, result.job_id, skipped)?;
            match outcome {
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
    }
    Ok(())
}
