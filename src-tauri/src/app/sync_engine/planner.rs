fn recompute_sync_file_actions(
    profile_id: &str,
    two_way_ready: bool,
) -> Result<SyncFilePlannerCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;

    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = 'none',
                 conflict_state = NULL
             WHERE profile_id = ?1",
            params![profile_id],
        )
        .map_err(|error| format!("Failed clearing sync file actions: {error}"))?;

    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = 'download'
             WHERE profile_id = ?1
               AND is_dir = 0
               AND is_shared_reference = 0
               AND remote_present = 1
               AND local_present = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM sync_jobs
                   WHERE sync_jobs.profile_id = sync_files.profile_id
                     AND sync_jobs.direction = 'download'
                     AND sync_jobs.item_id = sync_files.remote_item_id
                     AND sync_jobs.state = 'skipped'
               )",
            params![profile_id],
        )
        .map_err(|error| format!("Failed deriving download actions: {error}"))?;

    if two_way_ready {
        connection
            .execute(
                "UPDATE sync_files
                 SET desired_action = 'upload'
                 WHERE profile_id = ?1
                    AND is_dir = 0
                    AND is_shared_reference = 0
                    AND local_present = 1
                    AND remote_present = 0",
                params![profile_id],
            )
            .map_err(|error| format!("Failed deriving upload actions for local-only files: {error}"))?;
    }

    let local_dominates_action = if two_way_ready { "upload" } else { "none" };
    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = CASE
                 WHEN remote_modified_ts > local_modified_ts THEN 'download'
                 WHEN local_modified_ts > remote_modified_ts THEN ?2
                 WHEN remote_size != local_size THEN 'conflict'
                 ELSE 'none'
             END,
             conflict_state = CASE
                 WHEN remote_modified_ts = local_modified_ts AND remote_size != local_size
                     THEN 'metadata_mismatch'
                 ELSE NULL
             END
              WHERE profile_id = ?1
                AND is_dir = 0
                AND is_shared_reference = 0
                AND remote_present = 1
                AND local_present = 1",
            params![profile_id, local_dominates_action],
        )
        .map_err(|error| format!("Failed deriving overlap actions: {error}"))?;

    read_sync_file_planner_counters(profile_id)
}

fn read_sync_file_planner_counters(profile_id: &str) -> Result<SyncFilePlannerCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut counters = SyncFilePlannerCounters::default();

    counters.cloud_discovered_total = connection
        .query_row(
            "SELECT COUNT(1) FROM sync_files WHERE profile_id = ?1 AND remote_present = 1",
            params![profile_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Failed reading cloud discovered counter: {error}"))?
        .max(0) as usize;

    counters.local_discovered_total = connection
        .query_row(
            "SELECT COUNT(1) FROM sync_files WHERE profile_id = ?1 AND local_present = 1",
            params![profile_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Failed reading local discovered counter: {error}"))?
        .max(0) as usize;

    counters.shared_reference_total = connection
        .query_row(
            "SELECT COUNT(1) FROM sync_files WHERE profile_id = ?1 AND is_shared_reference = 1",
            params![profile_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Failed reading shared reference counter: {error}"))?
        .max(0) as usize;

    let mut statement = connection
        .prepare(
            "SELECT desired_action, COUNT(1)
             FROM sync_files
             WHERE profile_id = ?1
             GROUP BY desired_action",
        )
        .map_err(|error| format!("Failed preparing sync file action counters query: {error}"))?;

    let rows = statement
        .query_map(params![profile_id], |row| {
            let action: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((action, count.max(0) as usize))
        })
        .map_err(|error| format!("Failed querying sync file action counters: {error}"))?;

    for row in rows {
        let (action, count) =
            row.map_err(|error| format!("Failed reading sync file action counters: {error}"))?;
        match action.as_str() {
            "download" => counters.need_download_total = count,
            "upload" => counters.need_upload_total = count,
            "conflict" => counters.conflict_total = count,
            _ => {}
        }
    }

    Ok(counters)
}

fn list_sync_file_paths_by_desired_action(
    profile_id: &str,
    desired_action: &str,
) -> Result<Vec<String>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT path
             FROM sync_files
             WHERE profile_id = ?1
               AND desired_action = ?2
               AND is_dir = 0
               AND is_shared_reference = 0
               AND local_present = 1
             ORDER BY path ASC",
        )
        .map_err(|error| format!("Failed preparing sync file action path query: {error}"))?;

    let rows = statement
        .query_map(params![profile_id, desired_action], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed querying sync file action paths: {error}"))?;

    let mut paths: Vec<String> = Vec::new();
    for row in rows {
        paths.push(row.map_err(|error| format!("Failed reading sync file action path: {error}"))?);
    }
    Ok(paths)
}
