async fn process_remote_page_items(
    graph: &mut GraphContext,
    sync_root: &Path,
    sync_state: &mut PersistedSyncState,
    bootstrap_cloud_first: bool,
    cancel_flag: &Arc<AtomicBool>,
    items: Vec<DeltaItem>,
) -> Result<(), String> {
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
                sync_state.remote_by_id.remove(&item.id);
                sync_state.remote_path_to_id.remove(&existing.path);
                log::info!(
                    "{} [cycle:{}] REMOTE_DELETE_DEFERRED_TO_PLANNER path={} remote_id={}",
                    graph.account_prefix,
                    graph.cycle_id,
                    existing.path,
                    item.id
                );
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

        let shared_drive_id = shared_drive_id_from_delta_item(&item);
        let shared_item_id = shared_item_id_from_delta_item(&item);
        let shared_kind = shared_kind_from_delta_item(&item);
        let is_shared_reference = item.remote_item.is_some();

        let remote_entry = RemoteKnownItem {
            id: item.id.clone(),
            path: path.clone(),
            is_dir: item.folder.is_some(),
            size: item.size.unwrap_or(0),
            modified_ts: parse_rfc3339_seconds(item.last_modified_date_time.as_deref()),
            is_shared_reference,
            shared_drive_id,
            shared_item_id,
            shared_kind,
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
            upsert_remote_known_item(sync_state, remote_entry.clone());
            upsert_sync_file_remote_presence(&graph.profile_id, &remote_entry)?;
            continue;
        }

        if bootstrap_cloud_first {
            upsert_remote_known_item(sync_state, remote_entry.clone());
            upsert_sync_file_remote_presence(&graph.profile_id, &remote_entry)?;
            continue;
        }

        upsert_remote_known_item(sync_state, remote_entry.clone());
        upsert_sync_file_remote_presence(&graph.profile_id, &remote_entry)?;
    }

    Ok(())
}
