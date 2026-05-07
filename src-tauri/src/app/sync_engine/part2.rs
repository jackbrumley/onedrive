async fn apply_remote_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    changes: &[DeltaItem],
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_remote",
        "Applying remote changes",
    );
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
                    Ok(()) => Ok((item_id, path, remote_entry)),
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
            let (_, _, remote_entry) = task_result?;
            upsert_remote_known_item(sync_state, remote_entry);
            stats.downloaded_files += 1;
            completed_count += 1;
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

async fn apply_local_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    current_local_snapshot: &HashMap<String, LocalSnapshotEntry>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    runtime_set_phase(
        &graph.sync_runtime,
        &graph.profile_id,
        "applying_local",
        "Applying local changes",
    );
    let mut local_paths: Vec<String> = current_local_snapshot.keys().cloned().collect();
    local_paths.sort_by_key(|path| path.matches('/').count());
    for path in local_paths {
        ensure_not_cancelled(cancel_flag)?;
        let Some(local_entry) = current_local_snapshot.get(&path) else {
            continue;
        };
        let previous_local = sync_state.local_snapshot.get(&path);
        let local_changed = has_local_changed(local_entry, previous_local);
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

        if !local_changed {
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
            if local_entry.modified_ts >= remote_modified {
                log::info!(
                    "{} [cycle:{}] LOCAL_UPLOAD_EXISTING path={} remote_id={} local_ts={} remote_ts={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    path,
                    existing_id,
                    local_entry.modified_ts,
                    remote_modified
                );
                let uploaded = upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
                let known = remote_known_item_from_drive_item(uploaded, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.uploaded_files += 1;
            }
        } else {
            log::info!(
                "{} [cycle:{}] LOCAL_UPLOAD_NEW path={}",
                graph.account_prefix,
                graph.cycle_id,
                path
            );
            let uploaded = upload_file_by_path(graph, sync_root, &path, cancel_flag).await?;
            let known = remote_known_item_from_drive_item(uploaded, &path)?;
            upsert_remote_known_item(sync_state, known);
            stats.uploaded_files += 1;
        }
    }

    let deleted_paths: Vec<String> = sync_state
        .local_snapshot
        .keys()
        .filter(|path| !current_local_snapshot.contains_key(*path))
        .cloned()
        .collect();

    let mut deleted_paths = deleted_paths;
    deleted_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));

    for deleted_path in deleted_paths {
        ensure_not_cancelled(cancel_flag)?;
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

async fn graph_get_text(
    graph: &mut GraphContext,
    url: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        ensure_not_cancelled(cancel_flag)?;
        let response = tokio::select! {
            _ = wait_for_cancellation(Arc::clone(cancel_flag)) => {
                return Err(SYNC_CANCELLED_ERROR.to_string());
            }
            value = client
                .get(url)
                .bearer_auth(&graph.access_token)
                .send() => {
                value.map_err(|error| format!("Graph GET failed: {error}"))?
            }
        };
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading Graph response: {error}"))?;

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

async fn graph_delete(graph: &mut GraphContext, url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
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

