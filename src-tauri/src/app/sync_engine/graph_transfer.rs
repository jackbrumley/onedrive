async fn graph_get_text(
    graph: &mut GraphContext,
    url: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let client = graph_http_client()?;
    let mut refreshed = false;
    let mut attempt: u32 = 0;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        attempt += 1;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(url)
                .bearer_auth(&graph.access_token)
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
            graph.refresh_token().await?;
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

async fn graph_delete(graph: &mut GraphContext, url: &str) -> Result<(), String> {
    let client = graph_http_client()?;
    let mut refreshed = false;
    loop {
        let response = client
            .delete(url)
            .bearer_auth(&graph.access_token)
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
            graph.refresh_token().await?;
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
    let mut access_token = graph.access_token.clone();
    let mut token_refreshed = false;
    let mut attempt: u32 = 0;
    let transfer_id = runtime_start_transfer(
        &graph.sync_runtime,
        &graph.profile_id,
        "download",
        relative_path,
        None,
    );
    loop {
        ensure_not_cancelled(cancel_flag)?;
        attempt += 1;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                if let Some(active_transfer_id) = &transfer_id {
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
                .get(&url)
                .bearer_auth(&access_token)
                .send() => {
                value.map_err(|error| format!("Download request failed: {error}"))
            }
        };
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let delay = exponential_backoff_delay(attempt);
                    log::warn!(
                        "{} [cycle:{}] DOWNLOAD_RETRY attempt={} path={} reason={} delay_ms={}",
                        graph.account_prefix,
                        graph.cycle_id,
                        attempt,
                        relative_path,
                        error,
                        delay.as_millis()
                    );
                    sleep_with_cancellation(cancel_flag, delay).await?;
                    continue;
                }
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        &error,
                    );
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
            let refreshed = refresh_access_token(&graph.profile_id).await?;
            access_token = refreshed.access_token;
            token_refreshed = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            if attempt < MAX_DOWNLOAD_RETRIES {
                let delay = parse_retry_after_delay(response.headers())
                    .unwrap_or_else(|| exponential_backoff_delay(attempt));
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

        let temp_path = local_path.with_extension("somedrive-part");
        let mut output_file = std::fs::File::create(&temp_path).map_err(|error| {
            format!(
                "Failed creating temporary download file '{}': {}",
                temp_path.display(),
                error
            )
        })?;

        let mut stream = response.bytes_stream();
        let mut downloaded_bytes: u64 = 0;
        while let Some(chunk_result) = stream.next().await {
            if cancel_flag.load(Ordering::Relaxed) {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        SYNC_CANCELLED_ERROR,
                    );
                }
                let _ = std::fs::remove_file(&temp_path);
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            let chunk = match chunk_result {
                Ok(value) => value,
                Err(error) => {
                    if let Some(active_transfer_id) = &transfer_id {
                        runtime_finish_transfer_error(
                            &graph.sync_runtime,
                            &graph.profile_id,
                            active_transfer_id,
                            &format!("Failed reading download stream: {}", error),
                        );
                    }
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(format!("Failed reading download bytes stream: {error}"));
                }
            };

            if let Err(error) = output_file.write_all(&chunk) {
                if let Some(active_transfer_id) = &transfer_id {
                    runtime_finish_transfer_error(
                        &graph.sync_runtime,
                        &graph.profile_id,
                        active_transfer_id,
                        &format!("Failed writing download chunk: {}", error),
                    );
                }
                let _ = std::fs::remove_file(&temp_path);
                return Err(format!(
                    "Failed writing temporary file '{}': {}",
                    temp_path.display(),
                    error
                ));
            }

            downloaded_bytes += chunk.len() as u64;
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

        if let Err(error) = output_file.flush() {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Failed flushing temporary file: {}", error),
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Failed flushing temporary file '{}': {}",
                temp_path.display(),
                error
            ));
        }

        if cancel_flag.load(Ordering::Relaxed) {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    SYNC_CANCELLED_ERROR,
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(SYNC_CANCELLED_ERROR.to_string());
        }

        if let Err(error) = std::fs::rename(&temp_path, local_path) {
            if let Some(active_transfer_id) = &transfer_id {
                runtime_finish_transfer_error(
                    &graph.sync_runtime,
                    &graph.profile_id,
                    active_transfer_id,
                    &format!("Failed finalizing local file: {}", error),
                );
            }
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Failed moving '{}' to '{}': {}",
                temp_path.display(),
                local_path.display(),
                error
            ));
        }

        if let Some(active_transfer_id) = &transfer_id {
            runtime_update_transfer_progress(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
                downloaded_bytes,
                Some(downloaded_bytes),
            );
            runtime_finish_transfer_success(
                &graph.sync_runtime,
                &graph.profile_id,
                active_transfer_id,
            );
        }

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
        return upload_small_file_simple(
            graph,
            relative_path,
            content,
            content_len,
            transfer_id.as_deref(),
            cancel_flag,
        )
        .await;
    }

    upload_large_file_session(
        graph,
        relative_path,
        &local_path,
        content_len,
        transfer_id.as_deref(),
        cancel_flag,
    )
    .await
}

async fn upload_small_file_simple(
    graph: &mut GraphContext,
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
                .bearer_auth(&graph.access_token)
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
            graph.refresh_token().await?;
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
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .post(&url)
                .bearer_auth(&graph.access_token)
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
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
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
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .post(&endpoint)
                .bearer_auth(&graph.access_token)
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
            graph.refresh_token().await?;
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
