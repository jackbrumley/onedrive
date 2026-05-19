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
            is_shared_reference: false,
            shared_drive_id: None,
            shared_item_id: None,
            shared_kind: None,
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
    sync_root: &Path,
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
            match outcome {
                RemoteDownloadOutcome::Downloaded => {
                    let local_abs = sync_root.join(path_to_local(&result.remote_entry.path));
                    let local_entry = read_local_entry(&local_abs)?.ok_or_else(|| {
                        format!(
                            "Downloaded item missing on disk after completion path={} local_path={}",
                            result.remote_entry.path,
                            local_abs.display()
                        )
                    })?;
                    mark_download_job_done_with_local_index(
                        &graph.profile_id,
                        result.job_id,
                        &result.remote_entry,
                        &local_entry,
                    )?;
                    runtime_record_remote_download_completed(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        &result.remote_entry.id,
                    );
                    remote_applied_paths.insert(result.remote_entry.path.clone());
                    sync_state
                        .local_snapshot
                        .insert(result.remote_entry.path.clone(), local_entry);
                    upsert_remote_known_item(sync_state, result.remote_entry);
                    stats.downloaded_files += 1;
                }
                RemoteDownloadOutcome::SkippedMissingRemote => {
                    mark_download_job_done(&graph.profile_id, result.job_id, true)?;
                    sync_state.remote_by_id.remove(&result.remote_entry.id);
                    sync_state
                        .remote_path_to_id
                        .remove(&result.remote_entry.path);
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
