fn start_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let account_prefix = log_context::account_prefix(profile_id);
    let cancel_flag = set_cancel_flag(state, profile_id, false)?;
    let cycle_lock = get_or_create_cycle_lock(state, profile_id)?;
    let initial_delay = remaining_until_next_cycle(profile_id, Duration::from_secs(15));
    {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        if stops.contains_key(profile_id) {
            log::info!("{} SYNC_WORKER_ALREADY_RUNNING", account_prefix);
            return Ok(());
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        stops.insert(profile_id.to_string(), tx);
        log::info!(
            "{} SYNC_WORKER_STARTING interval_seconds=15",
            account_prefix
        );

        let profile_id_owned = profile_id.to_string();
        let profiles_lock = Arc::clone(&state.profiles_lock);
        let sync_runtime = Arc::clone(&state.sync_runtime);
        if let Ok(mut runtime_map) = sync_runtime.lock() {
            sync_runtime::set_phase(
                &mut runtime_map,
                &profile_id_owned,
                "syncing",
                "Preparing next sync cycle",
            );
            sync_runtime::set_remote_transfer_progress(&mut runtime_map, &profile_id_owned, 0, 0, 0);
            sync_runtime::clear_issue(&mut runtime_map, &profile_id_owned);
        }
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            if let Some(delay) = initial_delay {
                log::info!(
                    "{} SYNC_WORKER_RESUME_DELAY wait_ms={}",
                    log_context::account_prefix(&profile_id_owned),
                    delay.as_millis()
                );
                if sleep_with_cancellation(&cancel_flag, delay).await.is_err() {
                    if let Ok(mut runtime_map) = sync_runtime.lock() {
                        sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                        sync_runtime::set_remote_transfer_progress(&mut runtime_map, &profile_id_owned, 0, 0, 0);
                        sync_runtime::set_phase(
                            &mut runtime_map,
                            &profile_id_owned,
                            "paused",
                            "Synchronization paused",
                        );
                    }
                    return;
                }
            }
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        log::info!("{} SYNC_WORKER_STOP_SIGNAL", log_context::account_prefix(&profile_id_owned));
                        if let Ok(mut runtime_map) = sync_runtime.lock() {
                            sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                            sync_runtime::set_remote_transfer_progress(&mut runtime_map, &profile_id_owned, 0, 0, 0);
                            sync_runtime::set_phase(&mut runtime_map, &profile_id_owned, "paused", "Synchronization paused");
                        }
                        break;
                    }
                    _ = ticker.tick() => {
                        if cancel_flag.load(Ordering::Relaxed) {
                            continue;
                        }
                        let _cycle_guard = match cycle_lock.try_lock() {
                            Ok(guard) => guard,
                            Err(_) => {
                                log::warn!(
                                    "{} SYNC_TICK_SKIPPED cycle already running",
                                    log_context::account_prefix(&profile_id_owned)
                                );
                                continue;
                            }
                        };
                        log::info!("{} SYNC_TICK", log_context::account_prefix(&profile_id_owned));
                        match tick_sync_cycle(&profiles_lock, &sync_runtime, &profile_id_owned, &cancel_flag).await {
                            Ok(stats) => {
                                log::info!(
                                    "{} [cycle:{}] SYNC_CYCLE_COMPLETE downloaded={} uploaded={} upload_failures={} upload_cooldown_skips={} local_deleted={} remote_deleted={} remote_folders={} remote_pages={} remote_items={} remote_missing_skips={} local_items={}",
                                    stats.account_prefix,
                                    stats.cycle_id,
                                    stats.downloaded_files,
                                    stats.uploaded_files,
                                    stats.upload_failures,
                                    stats.upload_cooldown_skips,
                                    stats.deleted_local,
                                    stats.deleted_remote,
                                    stats.created_remote_folders,
                                    stats.remote_pages,
                                    stats.remote_items_received,
                                    stats.remote_items_skipped_missing,
                                    stats.local_items_seen,
                                );
                            }
                            Err(error) => {
                                if is_sync_cancelled_error(&error) {
                                    log::info!(
                                        "{} SYNC_CYCLE_CANCELLED",
                                        log_context::account_prefix(&profile_id_owned)
                                    );
                                    if let Ok(mut runtime_map) = sync_runtime.lock() {
                                        sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                                        sync_runtime::set_remote_transfer_progress(&mut runtime_map, &profile_id_owned, 0, 0, 0);
                                    }
                                    continue;
                                }
                                let (issue_code, issue_actions) = classify_sync_issue(&error);
                                log::error!(
                                    "{} SYNC_CYCLE_FAILED {}",
                                    log_context::account_prefix(&profile_id_owned),
                                    error
                                );
                                if let Ok(mut runtime_map) = sync_runtime.lock() {
                                    sync_runtime::clear_in_progress(&mut runtime_map, &profile_id_owned);
                                    sync_runtime::set_remote_transfer_progress(&mut runtime_map, &profile_id_owned, 0, 0, 0);
                                    sync_runtime::set_phase(
                                        &mut runtime_map,
                                        &profile_id_owned,
                                        "error",
                                        &format!("Sync error: {}", error),
                                    );
                                    sync_runtime::set_issue(
                                        &mut runtime_map,
                                        &profile_id_owned,
                                        issue_code,
                                        &error,
                                        issue_actions,
                                        None,
                                        None,
                                    );
                                }
                                let _ = activity_log::append_event(
                                    &profile_id_owned,
                                    &log_context::account_identity(&profile_id_owned),
                                    "error",
                                    &format!(
                                        "{} SYNC_CYCLE_FAILED {error}",
                                        log_context::account_prefix(&profile_id_owned)
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    let _ = activity_log::append_event(
        profile_id,
        &log_context::account_identity(profile_id),
        "info",
        &format!(
            "{} Sync agent started",
            log_context::account_prefix(profile_id)
        ),
    );
    Ok(())
}

fn stop_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let _ = set_cancel_flag(state, profile_id, true)?;
    let maybe_sender = {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        stops.remove(profile_id)
    };

    if let Some(sender) = maybe_sender {
        let _ = sender.send(());
        let _ = activity_log::append_event(
            profile_id,
            &log_context::account_identity(profile_id),
            "info",
            &format!(
                "{} Sync agent stopped",
                log_context::account_prefix(profile_id)
            ),
        );
    }

    Ok(())
}
