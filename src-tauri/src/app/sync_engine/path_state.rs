fn split_parent_and_name(relative_path: &str) -> Result<(&str, &str), String> {
    let trimmed = relative_path.trim_matches('/');
    if trimmed.is_empty() {
        return Err("Remote folder path is empty".to_string());
    }
    if let Some((parent, name)) = trimmed.rsplit_once('/') {
        return Ok((parent, name));
    }
    Ok(("", trimmed))
}

fn encode_graph_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        parts.push(percent_encode(segment));
    }
    parts.join("/")
}

fn percent_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        let unreserved = matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if unreserved {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push_str(&format!("{:02X}", byte));
        }
    }
    output
}

fn remote_known_item_from_drive_item(
    item: DriveItemResponse,
    fallback_path: &str,
) -> Result<RemoteKnownItem, String> {
    let path = if let (Some(name), Some(parent_ref)) =
        (item.name.clone(), item.parent_reference.clone())
    {
        let parent = parent_ref
            .path
            .as_deref()
            .map(extract_root_relative)
            .unwrap_or_default();
        normalize_relative_path(&if parent.is_empty() {
            name
        } else {
            format!("{}/{}", parent, name)
        })
    } else {
        fallback_path.to_string()
    };
    if path.trim().is_empty() {
        return Err("Remote item path cannot be empty".to_string());
    }

    Ok(RemoteKnownItem {
        id: item.id,
        path,
        is_dir: item.folder.is_some(),
        size: item.size.unwrap_or(0),
        modified_ts: parse_rfc3339_seconds(item.last_modified_date_time.as_deref()),
    })
}

fn collect_local_snapshot(sync_root: &Path) -> Result<HashMap<String, LocalSnapshotEntry>, String> {
    collect_local_snapshot_with_progress(sync_root, |_| {})
}

fn collect_local_snapshot_with_progress<F>(
    sync_root: &Path,
    mut on_progress: F,
) -> Result<HashMap<String, LocalSnapshotEntry>, String>
where
    F: FnMut(&str),
{
    let mut snapshot = HashMap::new();
    if !sync_root.exists() {
        return Ok(snapshot);
    }

    let mut stack: Vec<PathBuf> = vec![sync_root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|error| {
            format!(
                "Failed reading directory '{}': {}",
                current.display(),
                error
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = entry.metadata().map_err(|error| {
                format!("Failed reading metadata '{}': {}", path.display(), error)
            })?;

            if !metadata.is_file() && !metadata.is_dir() {
                continue;
            }

            let relative = path
                .strip_prefix(sync_root)
                .map_err(|error| {
                    format!(
                        "Failed computing relative path '{}': {}",
                        path.display(),
                        error
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            let normalized = normalize_relative_path(&relative);
            if normalized.is_empty() {
                continue;
            }

            let modified_ts = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|value| value.as_secs() as i64)
                .unwrap_or(0);

            on_progress(&normalized);

            snapshot.insert(
                normalized,
                LocalSnapshotEntry {
                    is_dir: metadata.is_dir(),
                    size: if metadata.is_file() {
                        metadata.len()
                    } else {
                        0
                    },
                    modified_ts,
                },
            );

            if metadata.is_dir() {
                stack.push(path);
            }
        }
    }

    Ok(snapshot)
}

fn read_local_entry(path: &Path) -> Result<Option<LocalSnapshotEntry>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let modified_ts = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0);
    Ok(Some(LocalSnapshotEntry {
        is_dir: metadata.is_dir(),
        size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        modified_ts,
    }))
}

fn sync_state_path(profile_id: &str) -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir
        .join("somedrive")
        .join("accounts")
        .join(profile_id)
        .join("sync_state.json"))
}

fn load_sync_state(profile_id: &str) -> Result<PersistedSyncState, String> {
    if let Some(state_json) = read_sync_state_store(profile_id)? {
        let mut state = serde_json::from_str::<PersistedSyncState>(&state_json)
            .map_err(|error| format!("Failed decoding persisted sync state: {error}"))?;
        let _ = hydrate_sync_state_from_lifecycle(profile_id, &mut state)?;
        return Ok(state);
    }

    let path = sync_state_path(profile_id)?;
    let mut state = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("Failed reading sync state '{}': {}", path.display(), error))?;
        serde_json::from_str::<PersistedSyncState>(&text)
            .map_err(|error| format!("Failed decoding sync state JSON: {error}"))?
    } else {
        PersistedSyncState::default()
    };

    let _ = hydrate_sync_state_from_lifecycle(profile_id, &mut state)?;
    persist_sync_lifecycle_from_state(profile_id, &state)?;

    let state_json = serde_json::to_string_pretty(&state)
        .map_err(|error| format!("Failed encoding sync state JSON: {error}"))?;
    write_sync_state_store(profile_id, &state_json)?;

    Ok(state)
}

fn save_sync_state(profile_id: &str, state: &PersistedSyncState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Failed encoding sync state JSON: {error}"))?;
    write_sync_state_store(profile_id, &text)?;

    persist_sync_lifecycle_from_state(profile_id, state)?;
    Ok(())
}
