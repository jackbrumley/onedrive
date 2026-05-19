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
            PLANNER_ACTION_DOWNLOAD => counters.need_download_total = count,
            PLANNER_ACTION_UPLOAD => counters.need_upload_total = count,
            PLANNER_ACTION_DELETE_REMOTE => counters.need_delete_remote_total = count,
            PLANNER_ACTION_DELETE_LOCAL => counters.need_delete_local_total = count,
            PLANNER_ACTION_CONFLICT => counters.conflict_total = count,
            _ => {}
        }
    }

    Ok(counters)
}

fn read_materialized_job_counts(profile_id: &str) -> Result<(usize, usize), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .query_row(
            "SELECT
                SUM(CASE
                    WHEN direction = 'download' AND state IN ('queued', 'in_progress', 'retry_wait') THEN 1
                    ELSE 0
                END),
                SUM(CASE
                    WHEN direction = 'upload' AND state IN ('queued', 'in_progress', 'retry_wait') THEN 1
                    ELSE 0
                END)
             FROM sync_jobs
             WHERE profile_id = ?1",
            params![profile_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as usize,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as usize,
                ))
            },
        )
        .map_err(|error| format!("Failed reading materialized job counts: {error}"))
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
