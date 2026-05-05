use crate::app::account_profiles::{load_profiles, save_profiles, AccountProfile};
use crate::app::activity_log;
use crate::app::auth::{load_auth_session, refresh_access_token};
use crate::app::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";

pub fn on_agent_state_changed(
    state: &tauri::State<'_, AppState>,
    profile_id: &str,
    agent_state: &str,
) -> Result<(), String> {
    if agent_state == "syncing" {
        start_sync_worker(state, profile_id)?;
    } else {
        stop_sync_worker(state, profile_id)?;
    }
    Ok(())
}

fn start_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        if stops.contains_key(profile_id) {
            return Ok(());
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        stops.insert(profile_id.to_string(), tx);

        let profile_id_owned = profile_id.to_string();
        let profiles_lock = Arc::clone(&state.profiles_lock);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        break;
                    }
                    _ = ticker.tick() => {
                        match tick_sync_cycle(&profiles_lock, &profile_id_owned).await {
                            Ok(stats) => {
                                log::info!(
                                    "Sync cycle profile_id={} downloaded={} uploaded={} local_deleted={} remote_deleted={} remote_folders={} remote_pages={} remote_items={} local_items={}",
                                    profile_id_owned,
                                    stats.downloaded_files,
                                    stats.uploaded_files,
                                    stats.deleted_local,
                                    stats.deleted_remote,
                                    stats.created_remote_folders,
                                    stats.remote_pages,
                                    stats.remote_items_received,
                                    stats.local_items_seen,
                                );
                            }
                            Err(error) => {
                                log::error!("Sync cycle failed for profile_id={}: {}", profile_id_owned, error);
                                let _ = activity_log::append_event(
                                    &profile_id_owned,
                                    &profile_id_owned,
                                    "error",
                                    &format!("Sync cycle failed: {error}"),
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    let _ = activity_log::append_event(profile_id, profile_id, "info", "Sync agent started");
    Ok(())
}

fn stop_sync_worker(state: &tauri::State<'_, AppState>, profile_id: &str) -> Result<(), String> {
    let maybe_sender = {
        let mut stops = state
            .sync_worker_stops
            .lock()
            .map_err(|_| "Sync worker lock is poisoned".to_string())?;
        stops.remove(profile_id)
    };

    if let Some(sender) = maybe_sender {
        let _ = sender.send(());
        let _ = activity_log::append_event(profile_id, profile_id, "info", "Sync agent stopped");
    }

    Ok(())
}

#[derive(Default)]
struct SyncCycleStats {
    downloaded_files: usize,
    uploaded_files: usize,
    deleted_local: usize,
    deleted_remote: usize,
    created_remote_folders: usize,
    remote_pages: usize,
    remote_items_received: usize,
    local_items_seen: usize,
}

struct GraphContext {
    profile_id: String,
    access_token: String,
}

impl GraphContext {
    async fn refresh_token(&mut self) -> Result<(), String> {
        let refreshed = refresh_access_token(&self.profile_id).await?;
        self.access_token = refreshed.access_token;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LocalSnapshotEntry {
    is_dir: bool,
    size: u64,
    modified_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteKnownItem {
    id: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified_ts: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSyncState {
    delta_link: Option<String>,
    remote_by_id: HashMap<String, RemoteKnownItem>,
    remote_path_to_id: HashMap<String, String>,
    local_snapshot: HashMap<String, LocalSnapshotEntry>,
    last_cycle_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeltaResponse {
    value: Vec<DeltaItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaItem {
    id: String,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    deleted: Option<serde_json::Value>,
    parent_reference: Option<ParentReference>,
    last_modified_date_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParentReference {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveItemResponse {
    id: String,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    parent_reference: Option<ParentReference>,
    last_modified_date_time: Option<String>,
}

async fn tick_sync_cycle(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<SyncCycleStats, String> {
    let profile = load_syncable_profile(profiles_lock, profile_id)?;
    let sync_root = PathBuf::from(profile.sync_root.clone());
    std::fs::create_dir_all(&sync_root)
        .map_err(|error| format!("Failed to create sync root '{}': {}", sync_root.display(), error))?;

    let session = load_auth_session(profile_id)?;
    if session.access_token.trim().is_empty() {
        return Err("Auth access token is empty; re-authentication required".to_string());
    }

    let mut graph = GraphContext {
        profile_id: profile_id.to_string(),
        access_token: session.access_token,
    };

    let mut sync_state = load_sync_state(profile_id)?;
    let mut stats = SyncCycleStats::default();
    let _ = activity_log::append_event(profile_id, &profile.display_name, "info", "Sync cycle started");

    let remote_changes = fetch_delta_changes(
        &mut graph,
        sync_state.delta_link.clone(),
        &mut sync_state,
        &mut stats,
    )
    .await?;

    apply_remote_changes(
        &mut graph,
        &sync_root,
        &remote_changes,
        &mut sync_state,
        &mut stats,
    )
    .await?;

    let local_snapshot = collect_local_snapshot(&sync_root)?;
    stats.local_items_seen = local_snapshot.len();
    apply_local_changes(
        &mut graph,
        &sync_root,
        &local_snapshot,
        &mut sync_state,
        &mut stats,
    )
    .await?;

    sync_state.local_snapshot = collect_local_snapshot(&sync_root)?;
    sync_state.last_cycle_at = Some(chrono::Local::now().to_rfc3339());
    save_sync_state(profile_id, &sync_state)?;

    update_profile_last_sync(profiles_lock, profile_id)?;

    let summary = format!(
        "Sync cycle complete (downloaded {}, uploaded {}, remote deletes {}, local deletes {}, remote pages {}, remote items {}, local items {})",
        stats.downloaded_files,
        stats.uploaded_files,
        stats.deleted_remote,
        stats.deleted_local,
        stats.remote_pages,
        stats.remote_items_received,
        stats.local_items_seen
    );
    let _ = activity_log::append_event(profile_id, &profile.display_name, "success", &summary);
    Ok(stats)
}

fn load_syncable_profile(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<AccountProfile, String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let profiles = load_profiles()?;
    let profile = profiles
        .into_iter()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    if profile.agent_state != "syncing" {
        return Err("Account is not in syncing state".to_string());
    }
    if !profile.auth_configured {
        return Err("Account is not authenticated".to_string());
    }
    Ok(profile)
}

fn update_profile_last_sync(
    profiles_lock: &Arc<std::sync::Mutex<()>>,
    profile_id: &str,
) -> Result<(), String> {
    let _guard = profiles_lock
        .lock()
        .map_err(|_| "Account profile lock is poisoned".to_string())?;
    let mut profiles = load_profiles()?;
    let profile = profiles
        .iter_mut()
        .find(|entry| entry.id == profile_id)
        .ok_or_else(|| "Account profile not found".to_string())?;
    profile.last_sync_at = Some(chrono::Local::now().to_rfc3339());
    save_profiles(&profiles)
}

async fn fetch_delta_changes(
    graph: &mut GraphContext,
    initial_delta_link: Option<String>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
) -> Result<Vec<DeltaItem>, String> {
    let mut all_items: Vec<DeltaItem> = Vec::new();
    let mut current_url = initial_delta_link
        .unwrap_or_else(|| format!("{GRAPH_ROOT}/me/drive/root/delta"));

    loop {
        let response_text = graph_get_text(graph, &current_url).await?;
        let response: DeltaResponse = serde_json::from_str(&response_text)
            .map_err(|error| format!("Failed to decode delta response: {error}"))?;

        stats.remote_pages += 1;
        stats.remote_items_received += response.value.len();
        all_items.extend(response.value.into_iter());

        if let Some(next_link) = response.next_link {
            current_url = next_link;
            continue;
        }

        if let Some(delta_link) = response.delta_link {
            sync_state.delta_link = Some(delta_link);
        }
        break;
    }

    Ok(all_items)
}

async fn apply_remote_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    changes: &[DeltaItem],
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
) -> Result<(), String> {
    for item in changes {
        if item.deleted.is_some() {
            if let Some(existing) = sync_state.remote_by_id.get(&item.id).cloned() {
                let local_abs = sync_root.join(path_to_local(&existing.path));
                let local_current = read_local_entry(&local_abs)?;
                let previous_local = sync_state.local_snapshot.get(&existing.path);
                let local_changed = local_current
                    .as_ref()
                    .map(|entry| has_local_changed(entry, previous_local))
                    .unwrap_or(false);

                if local_changed && !existing.is_dir {
                    let uploaded = upload_file_by_path(graph, sync_root, &existing.path).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &existing.path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                sync_state.remote_by_id.remove(&item.id);
                sync_state.remote_path_to_id.remove(&existing.path);
                sync_state.local_snapshot.remove(&existing.path);
                remove_local_path(sync_root, &existing.path)?;
                stats.deleted_local += 1;
            }
            continue;
        }

        let Some(path) = resolve_delta_item_path(item) else {
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
            std::fs::create_dir_all(&local_abs)
                .map_err(|error| format!("Failed creating local directory '{}': {}", local_abs.display(), error))?;
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
                    let uploaded = upload_file_by_path(graph, sync_root, &path).await?;
                    let known = remote_known_item_from_drive_item(uploaded, &path)?;
                    upsert_remote_known_item(sync_state, known);
                    stats.uploaded_files += 1;
                    continue;
                }

                create_safe_backup(&local_abs)?;
            }

            download_remote_item_content(graph, &item.id, &local_abs).await?;
            stats.downloaded_files += 1;
        }

        upsert_remote_known_item(sync_state, remote_entry);
    }

    Ok(())
}

async fn apply_local_changes(
    graph: &mut GraphContext,
    sync_root: &Path,
    current_local_snapshot: &HashMap<String, LocalSnapshotEntry>,
    sync_state: &mut PersistedSyncState,
    stats: &mut SyncCycleStats,
) -> Result<(), String> {
    let mut local_paths: Vec<String> = current_local_snapshot.keys().cloned().collect();
    local_paths.sort_by_key(|path| path.matches('/').count());
    for path in local_paths {
        let Some(local_entry) = current_local_snapshot.get(&path) else {
            continue;
        };
        let previous_local = sync_state.local_snapshot.get(&path);
        let local_changed = has_local_changed(local_entry, previous_local);
        let remote_id = sync_state.remote_path_to_id.get(&path).cloned();

        if local_entry.is_dir {
            if remote_id.is_none() {
                let created = create_remote_folder(graph, &path).await?;
                let known = remote_known_item_from_drive_item(created, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.created_remote_folders += 1;
            }
            continue;
        }

        if !local_changed {
            continue;
        }

        if let Some(existing_id) = remote_id {
            let remote_modified = sync_state
                .remote_by_id
                .get(&existing_id)
                .map(|item| item.modified_ts)
                .unwrap_or(0);
            if local_entry.modified_ts >= remote_modified {
                let uploaded = upload_file_by_path(graph, sync_root, &path).await?;
                let known = remote_known_item_from_drive_item(uploaded, &path)?;
                upsert_remote_known_item(sync_state, known);
                stats.uploaded_files += 1;
            }
        } else {
            let uploaded = upload_file_by_path(graph, sync_root, &path).await?;
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
        if let Some(remote_id) = sync_state.remote_path_to_id.get(&deleted_path).cloned() {
            delete_remote_item(graph, &remote_id).await?;
            sync_state.remote_path_to_id.remove(&deleted_path);
            sync_state.remote_by_id.remove(&remote_id);
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
        std::fs::remove_dir_all(&full_path)
            .map_err(|error| format!("Failed removing directory '{}': {}", full_path.display(), error))
    } else {
        std::fs::remove_file(&full_path)
            .map_err(|error| format!("Failed removing file '{}': {}", full_path.display(), error))
    }
}

fn create_safe_backup(local_path: &Path) -> Result<(), String> {
    if !local_path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(local_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Ok(());
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
            return Ok(());
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

async fn graph_get_text(graph: &mut GraphContext, url: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        let response = client
            .get(url)
            .bearer_auth(&graph.access_token)
            .send()
            .await
            .map_err(|error| format!("Graph GET failed: {error}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading Graph response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!("Graph GET unauthorized, refreshing token for profile_id={}", graph.profile_id);
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!("Graph GET {} failed with status {}: {}", url, status, snippet));
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
            log::warn!("Graph DELETE unauthorized, refreshing token for profile_id={}", graph.profile_id);
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }

        let text = response.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(400).collect();
        return Err(format!("Graph DELETE {} failed with status {}: {}", url, status, snippet));
    }
}

async fn download_remote_item_content(
    graph: &mut GraphContext,
    item_id: &str,
    local_path: &Path,
) -> Result<(), String> {
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}/content", item_id);
    let client = reqwest::Client::new();
    let mut refreshed = false;
    let bytes = loop {
        let response = client
            .get(&url)
            .bearer_auth(&graph.access_token)
            .send()
            .await
            .map_err(|error| format!("Failed to download remote content: {error}"))?;
        let status = response.status();
        if status.as_u16() == 401 && !refreshed {
            log::warn!("Graph download unauthorized, refreshing token for profile_id={}", graph.profile_id);
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Download failed for item {} with status {}: {}",
                item_id, status, snippet
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Failed reading download bytes: {error}"))?;
        break bytes;
    };

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed creating local parent '{}': {}", parent.display(), error))?;
    }
    std::fs::write(local_path, &bytes)
        .map_err(|error| format!("Failed writing local file '{}': {}", local_path.display(), error))
}

async fn upload_file_by_path(
    graph: &mut GraphContext,
    sync_root: &Path,
    relative_path: &str,
) -> Result<DriveItemResponse, String> {
    let local_path = sync_root.join(path_to_local(relative_path));
    let content = std::fs::read(&local_path)
        .map_err(|error| format!("Failed reading local file '{}': {}", local_path.display(), error))?;

    let encoded_path = encode_graph_path(relative_path);
    let url = format!("{GRAPH_ROOT}/me/drive/root:/{}:/content", encoded_path);
    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        let response = client
            .put(&url)
            .bearer_auth(&graph.access_token)
            .body(content.clone())
            .send()
            .await
            .map_err(|error| format!("Failed uploading file '{}': {}", relative_path, error))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading upload response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!("Graph upload unauthorized, refreshing token for profile_id={}", graph.profile_id);
            graph.refresh_token().await?;
            refreshed = true;
            continue;
        }

        if !status.is_success() {
            let snippet: String = text.chars().take(400).collect();
            return Err(format!(
                "Upload failed for '{}' with status {}: {}",
                relative_path, status, snippet
            ));
        }
        return serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding upload response JSON: {error}"));
    }
}

async fn create_remote_folder(graph: &mut GraphContext, relative_path: &str) -> Result<DriveItemResponse, String> {
    let (parent, name) = split_parent_and_name(relative_path)?;
    let endpoint = if parent.is_empty() {
        format!("{GRAPH_ROOT}/me/drive/root/children")
    } else {
        format!("{GRAPH_ROOT}/me/drive/root:/{}:/children", encode_graph_path(parent))
    };
    let payload = serde_json::json!({
        "name": name,
        "folder": {},
        "@microsoft.graph.conflictBehavior": "replace"
    });

    let client = reqwest::Client::new();
    let mut refreshed = false;
    loop {
        let response = client
            .post(&endpoint)
            .bearer_auth(&graph.access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|error| format!("Failed creating remote folder '{}': {}", relative_path, error))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Failed reading create folder response: {error}"))?;

        if status.as_u16() == 401 && !refreshed {
            log::warn!("Graph folder-create unauthorized, refreshing token for profile_id={}", graph.profile_id);
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
        return serde_json::from_str::<DriveItemResponse>(&text)
            .map_err(|error| format!("Failed decoding create-folder response JSON: {error}"));
    }
}

async fn delete_remote_item(graph: &mut GraphContext, item_id: &str) -> Result<(), String> {
    let url = format!("{GRAPH_ROOT}/me/drive/items/{}", item_id);
    graph_delete(graph, &url).await
}

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
    let path = if let (Some(name), Some(parent_ref)) = (item.name.clone(), item.parent_reference.clone()) {
        let parent = parent_ref.path.as_deref().map(extract_root_relative).unwrap_or_default();
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
    let mut snapshot = HashMap::new();
    if !sync_root.exists() {
        return Ok(snapshot);
    }

    let mut stack: Vec<PathBuf> = vec![sync_root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|error| format!("Failed reading directory '{}': {}", current.display(), error))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = entry
                .metadata()
                .map_err(|error| format!("Failed reading metadata '{}': {}", path.display(), error))?;

            if !metadata.is_file() && !metadata.is_dir() {
                continue;
            }

            let relative = path
                .strip_prefix(sync_root)
                .map_err(|error| format!("Failed computing relative path '{}': {}", path.display(), error))?
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

            snapshot.insert(
                normalized,
                LocalSnapshotEntry {
                    is_dir: metadata.is_dir(),
                    size: if metadata.is_file() { metadata.len() } else { 0 },
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
        size: if metadata.is_file() { metadata.len() } else { 0 },
        modified_ts,
    }))
}

fn sync_state_path(profile_id: &str) -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir
        .join("onedrive")
        .join("accounts")
        .join(profile_id)
        .join("sync_state.json"))
}

fn load_sync_state(profile_id: &str) -> Result<PersistedSyncState, String> {
    let path = sync_state_path(profile_id)?;
    if !path.exists() {
        return Ok(PersistedSyncState::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("Failed reading sync state '{}': {}", path.display(), error))?;
    serde_json::from_str::<PersistedSyncState>(&text)
        .map_err(|error| format!("Failed decoding sync state JSON: {error}"))
}

fn save_sync_state(profile_id: &str, state: &PersistedSyncState) -> Result<(), String> {
    let path = sync_state_path(profile_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed creating sync state directory '{}': {}", parent.display(), error))?;
    }
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Failed encoding sync state JSON: {error}"))?;
    std::fs::write(&path, text)
        .map_err(|error| format!("Failed writing sync state '{}': {}", path.display(), error))
}
