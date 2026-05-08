async fn graph_get_text(
    graph: &GraphContext,
    url: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let client = graph_http_client()?;
    let mut refreshed = false;
    let mut attempt: u32 = 0;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        attempt += 1;
        let access_token = graph.current_access_token().await;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(url)
                .bearer_auth(&access_token)
                .send() => {
                value.map_err(|error| format!("Graph GET failed: {error}"))
            }
        };
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                if attempt < MAX_DOWNLOAD_RETRIES && is_transient_request_error(&error) {
                    let delay = exponential_backoff_delay(attempt);
                    log::warn!(
                        "{} [cycle:{}] GRAPH_GET_RETRY attempt={} url={} reason={} delay_ms={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        attempt,
                        url,
                        error,
                        delay.as_millis()
                    );
                    sleep_with_cancellation(cancel_flag, delay).await?;
                    continue;
                }
                return Err(error);
            }
        };

        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            if status == StatusCode::TOO_MANY_REQUESTS {
                let _ = record_throttle_event(&graph.profile_id, DOWNLOAD_JOB_DIRECTION);
            }
            if attempt < MAX_DOWNLOAD_RETRIES {
                let delay = parse_retry_after_delay(response.headers())
                    .unwrap_or_else(|| exponential_backoff_delay(attempt));
                log::warn!(
                    "{} [cycle:{}] GRAPH_GET_RETRY_HTTP attempt={} status={} url={} delay_ms={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    attempt,
                    status,
                    url,
                    delay.as_millis()
                );
                sleep_with_cancellation(cancel_flag, delay).await?;
                continue;
            }
        }

        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading Graph response: {error}"));
        let text = match text {
            Ok(value) => value,
            Err(error) => {
                if attempt < MAX_DOWNLOAD_RETRIES && is_transient_request_error(&error) {
                    let delay = exponential_backoff_delay(attempt);
                    log::warn!(
                        "{} [cycle:{}] GRAPH_GET_RETRY_BODY attempt={} url={} reason={} delay_ms={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        attempt,
                        url,
                        error,
                        delay.as_millis()
                    );
                    sleep_with_cancellation(cancel_flag, delay).await?;
                    continue;
                }
                return Err(error);
            }
        };

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_GET_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Graph GET {} failed with status {}: {}",
                url, status, snippet
            ));
        }
        return Ok(text);
    }
}

fn is_transient_request_error(error: &str) -> bool {
    let normalized = error.to_lowercase();
    normalized.contains("timed out")
        || normalized.contains("timeout")
        || normalized.contains("connection reset")
        || normalized.contains("connection aborted")
        || normalized.contains("connection refused")
        || normalized.contains("tempor")
        || normalized.contains("dns")
        || normalized.contains("unreachable")
}

async fn graph_delete(graph: &GraphContext, url: &str) -> Result<(), String> {
    let client = graph_http_client()?;
    let mut refreshed = false;
    loop {
        let access_token = graph.current_access_token().await;
        let response = client
            .delete(url)
            .bearer_auth(&access_token)
            .send()
            .await
            .map_err(|error| format!("Graph DELETE failed: {error}"))?;
        let status = response.status();
        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DELETE_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            refreshed = true;
            continue;
        }

        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }

        let text = response.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(400).collect();
        return Err(format!(
            "Graph DELETE {} failed with status {}: {}",
            url, status, snippet
        ));
    }
}

async fn download_remote_item_content(
    graph: &GraphContext,
    job_id: Option<i64>,
    item_id: &str,
    relative_path: &str,
    local_path: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<RemoteDownloadOutcome, String> {
    ensure_not_cancelled(cancel_flag)?;
    log::info!(
        "{} [cycle:{}] DOWNLOAD_START item_id={} path={} local_path={}",
        graph.account_prefix,
        graph.cycle_id,
        item_id,
        relative_path,
        local_path.display()
    );
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}/content", item_id);
    let client = graph_http_client()?;
    let mut token_refreshed = false;
    let mut attempt: u32 = 0;
    let transfer_id = runtime_start_transfer(
        &graph.sync_runtime,
        &graph.profile_id,
        "download",
        relative_path,
        None,
    );
    if let Some(active_job_id) = job_id {
        let _ = mark_download_job_running(&graph.profile_id, active_job_id);
    }
    loop {
        ensure_not_cancelled(cancel_flag)?;
        attempt += 1;
        if let Some(active_job_id) = job_id {
            let _ = mark_download_job_running(&graph.profile_id, active_job_id);
        }
        let access_token = graph.current_access_token().await;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                runtime_finish_transfer_cancelled(&graph.sync_runtime, &graph.profile_id, &transfer_id);
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(&url)
                .bearer_auth(&access_token)
                .send() => {
                value.map_err(|error| format!("Download request failed: {error}"))
            }
        };
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                if handle_download_retry(
                    graph,
                    job_id,
                    cancel_flag,
                    &transfer_id,
                    attempt,
                    relative_path,
                    &error,
                    None,
                    &error,
                )
                .await?
                {
                    continue;
                }
                return Err(error);
            }
        };
        let status = response.status();
        if status.as_u16() == 401 && !token_refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DOWNLOAD_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            token_refreshed = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let _ = record_throttle_event(&graph.profile_id, DOWNLOAD_JOB_DIRECTION);
            let response_headers = response.headers().clone();
            let response_text = response.text().await.unwrap_or_default();
            let delay = parse_retry_after_delay(&response_headers)
                .or_else(|| parse_retry_after_seconds_from_json_body(&response_text))
                .unwrap_or_else(|| exponential_backoff_delay(attempt));
            if let Some(active_job_id) = job_id {
                let retry_reason = format!("Download retry scheduled for HTTP status {status}");
                if let Err(error) = mark_download_job_retry_wait(
                    &graph.profile_id,
                    active_job_id,
                    &retry_reason,
                    delay,
                ) {
                    log::warn!(
                        "{} [cycle:{}] DOWNLOAD_RETRY_WAIT_PERSIST_FAILED path={} error={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        relative_path,
                        error
                    );
                }
            }
            runtime_finish_transfer_cancelled(&graph.sync_runtime, &graph.profile_id, &transfer_id);
            log::warn!(
                "{} [cycle:{}] DOWNLOAD_RETRY_DEFERRED status={} path={} delay_ms={}",
                graph.account_prefix,
                graph.cycle_id,
                status,
                relative_path,
                delay.as_millis()
            );
            return Err(DOWNLOAD_RETRY_DEFERRED_ERROR.to_string());
        }
        if status.is_server_error() {
            if attempt < MAX_DOWNLOAD_RETRIES {
                let delay = parse_retry_after_delay(response.headers())
                    .unwrap_or_else(|| exponential_backoff_delay(attempt));
                if let Some(active_job_id) = job_id {
                    let retry_reason = format!("Download retry scheduled for HTTP status {status}");
                    if let Err(error) = mark_download_job_retry_wait(
                        &graph.profile_id,
                        active_job_id,
                        &retry_reason,
                        delay,
                    ) {
                        log::warn!(
                            "{} [cycle:{}] DOWNLOAD_RETRY_WAIT_PERSIST_FAILED path={} error={}",
                            graph.account_prefix,
                            graph.cycle_id,
                            relative_path,
                            error
                        );
                    }
                }
                log::warn!(
                    "{} [cycle:{}] DOWNLOAD_RETRY_HTTP attempt={} status={} path={} delay_ms={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    attempt,
                    status,
                    relative_path,
                    delay.as_millis()
                );
                sleep_with_cancellation(cancel_flag, delay).await?;
                continue;
            }
        }
        if !status.is_success() {
            if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_success(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                    );
                }
                log::warn!(
                    "{} [cycle:{}] DOWNLOAD_SKIPPED_MISSING item_id={} path={} status={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    item_id,
                    relative_path,
                    status
                );
                return Ok(RemoteDownloadOutcome::SkippedMissingRemote);
            }
            let text = response.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(400).collect();
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Download failed with status {}", status),
                );
            }
            return Err(format!(
                "Download failed for item {} with status {}: {}",
                item_id, status, snippet
            ));
        }

        let total = response.content_length();
        if let Some(active_job_id) = job_id {
            let _ = update_download_job_progress(&graph.profile_id, active_job_id, 0, total);
        }
        if let Some(active_transfer_id) = &transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                0,
                total,
            );
        }

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed creating local parent '{}': {}",
                    parent.display(),
                    error
                )
            })?;
        }

        let temp_path = build_unique_download_temp_path(local_path, item_id, relative_path, &graph.cycle_id);
        let _ = std::fs::remove_file(&temp_path);
        let mut output_file = std::fs::File::create(&temp_path).map_err(|error| {
            format!(
                "Failed creating temporary download file '{}': {}",
                temp_path.display(),
                error
            )
        })?;

        let mut stream = response.bytes_stream();
        let mut downloaded_bytes: u64 = 0;
        let mut should_retry_download = false;
        while let Some(chunk_result) = stream.next().await {
            if cancel_flag.load(Ordering::Relaxed) {
                runtime_finish_transfer_cancelled(&graph.sync_runtime, &graph.profile_id, &transfer_id);
                let _ = std::fs::remove_file(&temp_path);
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            let chunk = match chunk_result {
                Ok(value) => value,
                Err(error) => {
                    let reason = format!("Failed reading download stream: {error}");
                    if handle_download_retry(
                        graph,
                        job_id,
                        cancel_flag,
                        &transfer_id,
                        attempt,
                        relative_path,
                        &reason,
                        None,
                        &reason,
                    )
                    .await?
                    {
                        should_retry_download = true;
                        break;
                    }
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(format!("Failed reading download bytes stream: {error}"));
                }
            };

            if let Err(error) = output_file.write_all(&chunk) {
                let reason = format!("Failed writing download chunk: {error}");
                if handle_download_retry(
                    graph,
                    job_id,
                    cancel_flag,
                    &transfer_id,
                    attempt,
                    relative_path,
                    &reason,
                    None,
                    &reason,
                )
                .await?
                {
                    should_retry_download = true;
                    break;
                }
                let _ = std::fs::remove_file(&temp_path);
                return Err(format!(
                    "Failed writing temporary file '{}': {}",
                    temp_path.display(),
                    error
                ));
            }

            downloaded_bytes += chunk.len() as u64;
            if let Some(active_job_id) = job_id {
                let _ = update_download_job_progress(
                    &graph.profile_id,
                    active_job_id,
                    downloaded_bytes,
                    total,
                );
            }
            if let Some(active_transfer_id) = &transfer_id {
                runtime_update_transfer_progress(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    downloaded_bytes,
                    total,
                );
            }
        }

        if should_retry_download {
            drop(output_file);
            let _ = std::fs::remove_file(&temp_path);
            continue;
        }

        if let Err(error) = output_file.flush() {
            drop(output_file);
            let reason = format!("Failed flushing temporary file: {error}");
            if handle_download_retry(
                graph,
                job_id,
                cancel_flag,
                &transfer_id,
                attempt,
                relative_path,
                &reason,
                Some(&temp_path),
                &reason,
            )
            .await?
            {
                continue;
            }
            return Err(format!(
                "Failed flushing temporary file '{}': {}",
                temp_path.display(),
                error
            ));
        }

        if cancel_flag.load(Ordering::Relaxed) {
            runtime_finish_transfer_cancelled(&graph.sync_runtime, &graph.profile_id, &transfer_id);
            let _ = std::fs::remove_file(&temp_path);
            return Err(SYNC_CANCELLED_ERROR.to_string());
        }

        if let Err(error) = std::fs::rename(&temp_path, local_path) {
            let reason = format!(
                "Failed finalizing local file: {} (temp_path={})",
                error,
                temp_path.display()
            );
            if handle_download_retry(
                graph,
                job_id,
                cancel_flag,
                &transfer_id,
                attempt,
                relative_path,
                &reason,
                Some(&temp_path),
                &reason,
            )
            .await?
            {
                continue;
            }
            return Err(format!(
                "Failed moving '{}' to '{}': {}",
                temp_path.display(),
                local_path.display(),
                error
            ));
        }

        runtime_finish_transfer_download_success(
            &graph.sync_runtime,
            &graph.profile_id,
            &transfer_id,
            downloaded_bytes,
        );

        if attempt > 1 {
            log::info!(
                "{} [cycle:{}] DOWNLOAD_RETRY_RECOVERED attempts={} path={}",
                graph.account_prefix,
                graph.cycle_id,
                attempt,
                relative_path
            );
        }

        log::info!(
            "{} [cycle:{}] DOWNLOAD_OK item_id={} path={} bytes={}",
            graph.account_prefix,
            graph.cycle_id,
            item_id,
            relative_path,
            downloaded_bytes
        );
        return Ok(RemoteDownloadOutcome::Downloaded);
    }
}

async fn upload_file_by_path(
    graph: &mut GraphContext,
    sync_root: &Path,
    relative_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    ensure_not_cancelled(cancel_flag)?;
    let local_path = sync_root.join(path_to_local(relative_path));
    let metadata = std::fs::metadata(&local_path).map_err(|error| {
        format!(
            "Failed reading local file metadata '{}': {}",
            local_path.display(),
            error
        )
    })?;
    let content_len = metadata.len();
    let modified_ts = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0);
    let upload_job_id = begin_upload_job(
        &graph.profile_id,
        relative_path,
        content_len,
        modified_ts,
        &graph.cycle_id,
    )?;
    sync_upload_counters_from_db(graph);
    let transfer_id = runtime_start_transfer(
        &graph.sync_runtime,
        &graph.profile_id,
        "upload",
        relative_path,
        Some(content_len),
    );
    log::info!(
        "{} [cycle:{}] UPLOAD_START path={} local_path={} bytes={}",
        graph.account_prefix,
        graph.cycle_id,
        relative_path,
        local_path.display(),
        content_len
    );

    let simple_upload_limit = resolve_simple_upload_max_bytes();
    if content_len <= simple_upload_limit {
        let content = std::fs::read(&local_path).map_err(|error| {
            format!(
                "Failed reading local file '{}': {}",
                local_path.display(),
                error
            )
        })?;
        let result = upload_small_file_simple(
            graph,
            upload_job_id,
            relative_path,
            content,
            content_len,
            transfer_id.as_deref(),
            cancel_flag,
        )
        .await;
        if let Err(error) = &result {
            if is_sync_cancelled_error(error) {
                let _ = mark_upload_job_retry_wait(
                    &graph.profile_id,
                    upload_job_id,
                    "Upload cancelled due to pause; retry scheduled on resume",
                    Duration::from_secs(1),
                );
            } else {
                let _ = mark_upload_job_failed(&graph.profile_id, upload_job_id, error);
            }
        } else {
            let _ = mark_upload_job_done(&graph.profile_id, upload_job_id);
        }
        sync_upload_counters_from_db(graph);
        return result;
    }

    let result = upload_large_file_session(
        graph,
        upload_job_id,
        relative_path,
        &local_path,
        content_len,
        transfer_id.as_deref(),
        cancel_flag,
    )
    .await;
    if let Err(error) = &result {
        if is_sync_cancelled_error(error) {
            let _ = mark_upload_job_retry_wait(
                &graph.profile_id,
                upload_job_id,
                "Upload cancelled due to pause; retry scheduled on resume",
                Duration::from_secs(1),
            );
        } else {
            let _ = mark_upload_job_failed(&graph.profile_id, upload_job_id, error);
        }
    } else {
        let _ = mark_upload_job_done(&graph.profile_id, upload_job_id);
    }
    sync_upload_counters_from_db(graph);
    result
}

fn sync_upload_counters_from_db(graph: &GraphContext) {
    match read_upload_job_counters(&graph.profile_id) {
        Ok(counters) => runtime_set_upload_counters(
            &graph.sync_runtime,
            &graph.profile_id,
            counters.planned_total,
            counters.completed,
            counters.failed_terminal,
            counters.in_progress,
        ),
        Err(error) => {
            log::warn!(
                "{} [cycle:{}] UPLOAD_COUNTERS_DB_SYNC_FAILED error={}",
                graph.account_prefix,
                graph.cycle_id,
                error
            );
        }
    }
}

async fn upload_small_file_simple(
    graph: &mut GraphContext,
    upload_job_id: i64,
    relative_path: &str,
    content: Vec<u8>,
    content_len: u64,
    transfer_id: Option<&str>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    let encoded_path = encode_graph_path(relative_path);
    let url = format!("{GRAPH_ROOT}/me/drive/root:/{}:/content", encoded_path);
    let client = graph_http_client()?;
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let access_token = graph.current_access_token().await;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                if let Some(active_transfer_id) = transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        SYNC_CANCELLED_ERROR,
                    );
                }
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .put(&url)
                .bearer_auth(&access_token)
                .body(content.clone())
                .send() => {
                value.map_err(|error| {
                    if let Some(active_transfer_id) = transfer_id {
                        runtime_finish_transfer_error(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            active_transfer_id,
                            &format!("Upload request failed: {}", error),
                        );
                    }
                    format!("Failed uploading file '{}': {}", relative_path, error)
                })?
            }
        };

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading upload response: {error}"))?;

        ensure_not_cancelled(cancel_flag)?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_UPLOAD_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            if let Some(active_transfer_id) = transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Upload failed with status {}", status),
                );
            }
            return Err(format!(
                "Upload failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
        let parsed = serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding upload response JSON: {error}"))?;
        let _ = update_upload_job_progress(
            &graph.profile_id,
            upload_job_id,
            content_len,
            Some(content_len),
        );
        if let Some(active_transfer_id) = transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                content_len,
                Some(content_len),
            );
            runtime_finish_transfer_success(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
            );
        }
        log::info!(
            "{} [cycle:{}] UPLOAD_OK path={} remote_id={} size={}",
            graph.account_prefix,
            graph.cycle_id,
            relative_path,
            parsed.id,
            parsed.size.unwrap_or(0)
        );
        return Ok(parsed);
    }
}

async fn upload_large_file_session(
    graph: &mut GraphContext,
    upload_job_id: i64,
    relative_path: &str,
    local_path: &Path,
    content_len: u64,
    transfer_id: Option<&str>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    let upload_url = create_upload_session(graph, relative_path, cancel_flag).await?;
    let chunk_size = resolve_upload_chunk_bytes();
    let client = graph_http_client()?;
    let mut file = std::fs::File::open(local_path).map_err(|error| {
        format!(
            "Failed opening local upload file '{}': {}",
            local_path.display(),
            error
        )
    })?;
    let mut offset: u64 = 0;

    while offset < content_len {
        ensure_not_cancelled(cancel_flag)?;
        let remaining = (content_len - offset) as usize;
        let read_len = remaining.min(chunk_size);
        let mut chunk = vec![0_u8; read_len];
        file.read_exact(&mut chunk).map_err(|error| {
            format!(
                "Failed reading upload chunk '{}': {}",
                local_path.display(),
                error
            )
        })?;

        let chunk_start = offset;
        let chunk_end = offset + read_len as u64 - 1;
        let content_range = format!("bytes {}-{}/{}", chunk_start, chunk_end, content_len);

        let mut attempt: u32 = 0;
        loop {
            ensure_not_cancelled(cancel_flag)?;
            attempt += 1;
            let response = tokio::select! {
                _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                    if let Some(active_transfer_id) = transfer_id {
                        runtime_finish_transfer_error(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            active_transfer_id,
                            SYNC_CANCELLED_ERROR,
                        );
                    }
                    return Err(SYNC_CANCELLED_ERROR.to_string());
                }
                value = client
                    .put(&upload_url)
                    .header(reqwest::header::CONTENT_LENGTH, read_len)
                    .header(reqwest::header::CONTENT_RANGE, content_range.clone())
                    .body(chunk.clone())
                    .send() => {
                    value.map_err(|error| format!("Upload chunk request failed: {error}"))?
                }
            };

            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if status == StatusCode::TOO_MANY_REQUESTS {
                    let _ = record_throttle_event(&graph.profile_id, UPLOAD_JOB_DIRECTION);
                }
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let delay = parse_retry_after_delay(response.headers())
                        .unwrap_or_else(|| exponential_backoff_delay(attempt));
                    log::warn!(
                        "{} [cycle:{}] UPLOAD_CHUNK_RETRY attempt={} status={} path={} range={} delay_ms={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        attempt,
                        status,
                        relative_path,
                        content_range,
                        delay.as_millis()
                    );
                    sleep_with_cancellation(cancel_flag, delay).await?;
                    continue;
                }
            }

            let text = response.text().await.unwrap_or_default();
            if status == StatusCode::ACCEPTED {
                offset += read_len as u64;
                let _ = update_upload_job_progress(
                    &graph.profile_id,
                    upload_job_id,
                    offset,
                    Some(content_len),
                );
                if let Some(active_transfer_id) = transfer_id {
                    runtime_update_transfer_progress(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        offset,
                        Some(content_len),
                    );
                }
                break;
            }

            if status.is_success() {
                let parsed = serde_json::from_str::<DriveItemResponse>(&text).map_err(|error| {
                    format!("Failed decoding upload-session completion response JSON: {error}")
                })?;
                let _ = update_upload_job_progress(
                    &graph.profile_id,
                    upload_job_id,
                    content_len,
                    Some(content_len),
                );
                if let Some(active_transfer_id) = transfer_id {
                    runtime_update_transfer_progress(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        content_len,
                        Some(content_len),
                    );
                    runtime_finish_transfer_success(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                    );
                }
                log::info!(
                    "{} [cycle:{}] UPLOAD_OK path={} remote_id={} size={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    relative_path,
                    parsed.id,
                    parsed.size.unwrap_or(0)
                );
                return Ok(parsed);
            }

            if let Some(active_transfer_id) = transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Upload failed with status {}", status),
                );
            }
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Upload failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
    }

    Err(format!(
        "Upload session ended before completion for '{}'",
        relative_path
    ))
}

async fn create_upload_session(
    graph: &mut GraphContext,
    relative_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let encoded_path = encode_graph_path(relative_path);
    let url = format!(
        "{GRAPH_ROOT}/me/drive/root:/{}:/createUploadSession",
        encoded_path
    );
    let payload = serde_json::json!({
        "item": {
            "@microsoft.graph.conflictBehavior": "replace"
        }
    });
    let client = graph_http_client()?;
    let mut refreshed = false;

    loop {
        ensure_not_cancelled(cancel_flag)?;
        let access_token = graph.current_access_token().await;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .post(&url)
                .bearer_auth(&access_token)
                .json(&payload)
                .send() => {
                value.map_err(|error| format!("Create upload session request failed: {error}"))?
            }
        };

        let status = response.status();
        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_UPLOAD_SESSION_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            refreshed = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            if status == StatusCode::TOO_MANY_REQUESTS {
                let _ = record_throttle_event(&graph.profile_id, UPLOAD_JOB_DIRECTION);
            }
            let delay = parse_retry_after_delay(response.headers()).unwrap_or_else(|| Duration::from_secs(2));
            log::warn!(
                "{} [cycle:{}] GRAPH_UPLOAD_SESSION_RETRY status={} path={} delay_ms={}",
                graph.account_prefix,
                graph.cycle_id,
                status,
                relative_path,
                delay.as_millis()
            );
            sleep_with_cancellation(cancel_flag, delay).await?;
            continue;
        }

        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading create upload session response: {error}"))?;
        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Create upload session failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }

        let session = serde_json::from_str::<UploadSessionResponse>(&text)
            .map_err(|error| format!("Failed decoding create upload session response JSON: {error}"))?;
        return Ok(session.upload_url);
    }
}

async fn create_remote_folder(
    graph: &mut GraphContext,
    relative_path: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<DriveItemResponse, String> {
    ensure_not_cancelled(cancel_flag)?;
    let (parent, name) = split_parent_and_name(relative_path)?;
    let endpoint = if parent.is_empty() {
        format!("{GRAPH_ROOT}/me/drive/root/children")
    } else {
        format!(
            "{GRAPH_ROOT}/me/drive/root:/{}:/children",
            encode_graph_path(parent)
        )
    };
    let payload = serde_json::json!({
        "name": name,
        "folder": {},
        "@microsoft.graph.conflictBehavior": "replace"
    });
    log::info!(
        "{} [cycle:{}] REMOTE_DIR_CREATE_REQUEST path={} parent={}",
        graph.account_prefix,
        graph.cycle_id,
        relative_path,
        parent
    );

    let client = graph_http_client()?;
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let access_token = graph.current_access_token().await;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .post(&endpoint)
                .bearer_auth(&access_token)
                .json(&payload)
                .send() => {
                value.map_err(|error| {
                    format!(
                        "Failed creating remote folder '{}': {}",
                        relative_path, error
                    )
                })?
            }
        };

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading create folder response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!(
                "{} [cycle:{}] GRAPH_DIR_CREATE_401_REFRESH",
                graph.account_prefix,
                graph.cycle_id
            );
            let _ = graph.refresh_token_if_needed(&access_token).await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Create folder failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
        let parsed = serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding create-folder response JSON: {error}"))?;
        log::info!(
            "{} [cycle:{}] REMOTE_DIR_CREATE_OK path={} remote_id={}",
            graph.account_prefix,
            graph.cycle_id,
            relative_path,
            parsed.id
        );
        return Ok(parsed);
    }
}

async fn delete_remote_item(
    graph: &mut GraphContext,
    item_id: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    ensure_not_cancelled(cancel_flag)?;
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}", item_id);
    log::info!(
        "{} [cycle:{}] REMOTE_DELETE_REQUEST item_id={}",
        graph.account_prefix,
        graph.cycle_id,
        item_id
    );
    graph_delete(graph, &url).await?;
    log::info!(
        "{} [cycle:{}] REMOTE_DELETE_RESULT item_id={} status=ok",
        graph.account_prefix,
        graph.cycle_id,
        item_id
    );
    Ok(())
}
