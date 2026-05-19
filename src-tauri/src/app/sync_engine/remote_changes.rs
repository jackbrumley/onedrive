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

    let mut lifecycle_state = read_sync_lifecycle_operational_state(&graph.profile_id)?;
    let bootstrap_requires_authoritative_scan =
        !lifecycle_state.two_way_ready && !lifecycle_state.bootstrap_full_scan_completed;
    let start_url = if bootstrap_requires_authoritative_scan {
        if let Some(active_next_link) = lifecycle_state.active_delta_next_link.clone() {
            active_next_link
        } else {
            if !lifecycle_state.bootstrap_scan_initialized {
                log::info!(
                    "{} [cycle:{}] BOOTSTRAP_SCAN_START mode=authoritative_root_delta",
                    graph.account_prefix,
                    graph.cycle_id
                );
            }
            lifecycle_state.bootstrap_scan_initialized = true;
            persist_sync_lifecycle_operational_state(&graph.profile_id, &lifecycle_state)?;
            format!("{GRAPH_ROOT}/me/drive/root/delta")
        }
    } else {
        lifecycle_state
            .active_delta_next_link
            .clone()
            .or_else(|| lifecycle_state.delta_link.clone())
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
    let producer_alive_flag = Arc::clone(&producer_alive);
    spawn_delta_page_producer(
        graph,
        start_url,
        Arc::clone(&cycle_cancel_flag),
        page_tx,
        producer_alive_flag,
    );

    let (download_tx_raw, download_rx) = mpsc::channel::<RemoteDownloadJob>(download_queue_capacity);
    let mut download_tx = Some(download_tx_raw);
    let (download_result_tx, mut download_result_rx) =
        mpsc::channel::<RemoteDownloadResult>(download_queue_capacity);
    let download_rx = Arc::new(tokio::sync::Mutex::new(download_rx));
    let pending_download_count = Arc::new(AtomicUsize::new(0));

    spawn_remote_download_workers(
        graph,
        download_concurrency,
        &cycle_cancel_flag,
        Arc::clone(&download_rx),
        download_result_tx.clone(),
        Arc::clone(&active_download_workers),
        Arc::clone(&pending_download_count),
    );
    drop(download_result_tx);

    let dispatcher_stop = Arc::new(AtomicBool::new(false));
    let dispatcher_stop_flag = Arc::clone(&dispatcher_stop);
    let dispatcher_download_tx = download_tx
        .as_ref()
        .ok_or_else(|| "Remote download queue unavailable".to_string())?
        .clone();
    spawn_remote_download_dispatcher(
        graph,
        sync_root,
        download_concurrency,
        Arc::clone(&pending_download_count),
        dispatcher_stop_flag,
        dispatcher_download_tx,
    );

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
                    sync_root,
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
                    !lifecycle_state.two_way_ready,
                    &cycle_cancel_flag,
                    page_payload.items,
                )
                .await;
                if let Err(error) = process_result {
                    terminal_error = Some(error);
                    break;
                }
                let _ = recompute_sync_file_actions(&graph.profile_id, lifecycle_state.two_way_ready)?;
                let _ = materialize_planner_download_jobs(&graph.profile_id)?;
                let _ = sync_download_counters_from_db(graph)?;

                if let Some(next_link) = page_payload.next_link {
                    lifecycle_state.active_delta_next_link = Some(next_link);
                    persist_sync_lifecycle_operational_state(&graph.profile_id, &lifecycle_state)?;
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
        lifecycle_state.delta_link = Some(delta_link);
    }
    lifecycle_state.active_delta_next_link = None;
    if !lifecycle_state.two_way_ready {
        lifecycle_state.bootstrap_full_scan_completed = true;
        lifecycle_state.bootstrap_scan_initialized = true;
    }
    persist_sync_lifecycle_operational_state(&graph.profile_id, &lifecycle_state)?;
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
