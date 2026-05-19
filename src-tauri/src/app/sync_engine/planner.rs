const PLANNER_ACTION_NONE: &str = "none";
const PLANNER_ACTION_DOWNLOAD: &str = "download";
const PLANNER_ACTION_UPLOAD: &str = "upload";
const PLANNER_ACTION_DELETE_REMOTE: &str = "delete_remote";
const PLANNER_ACTION_DELETE_LOCAL: &str = "delete_local";
const PLANNER_ACTION_CONFLICT: &str = "conflict";

fn recompute_sync_file_actions(
    profile_id: &str,
    two_way_ready: bool,
) -> Result<SyncFilePlannerCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;

    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = ?2,
                 conflict_state = NULL
             WHERE profile_id = ?1",
            params![profile_id, PLANNER_ACTION_NONE],
        )
        .map_err(|error| format!("Failed clearing sync file actions: {error}"))?;

    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = ?2
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
            params![profile_id, PLANNER_ACTION_DOWNLOAD],
        )
        .map_err(|error| format!("Failed deriving download actions: {error}"))?;

    if two_way_ready {
        connection
            .execute(
                "UPDATE sync_files
                 SET desired_action = ?2
                 WHERE profile_id = ?1
                    AND is_dir = 0
                    AND is_shared_reference = 0
                    AND local_present = 1
                    AND remote_present = 0",
                params![profile_id, PLANNER_ACTION_UPLOAD],
            )
            .map_err(|error| format!("Failed deriving upload actions for local-only files: {error}"))?;
    }

    let local_dominates_action = if two_way_ready {
        PLANNER_ACTION_UPLOAD
    } else {
        PLANNER_ACTION_NONE
    };
    connection
        .execute(
            "UPDATE sync_files
             SET desired_action = CASE
                 WHEN remote_modified_ts > local_modified_ts THEN ?3
                 WHEN local_modified_ts > remote_modified_ts THEN ?2
                 WHEN remote_size != local_size THEN ?4
                 ELSE ?5
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
            params![
                profile_id,
                local_dominates_action,
                PLANNER_ACTION_DOWNLOAD,
                PLANNER_ACTION_CONFLICT,
                PLANNER_ACTION_NONE,
            ],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile_id(label: &str) -> String {
        format!("planner-test-{label}-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default())
    }

    fn clear_profile_rows(profile_id: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute("DELETE FROM sync_files WHERE profile_id = ?1", params![profile_id])
            .expect("clear sync_files");
        connection
            .execute("DELETE FROM sync_jobs WHERE profile_id = ?1", params![profile_id])
            .expect("clear sync_jobs");
    }

    fn insert_sync_file_row(
        profile_id: &str,
        path: &str,
        remote_present: bool,
        local_present: bool,
        remote_modified_ts: i64,
        local_modified_ts: i64,
        remote_size: u64,
        local_size: u64,
        remote_item_id: Option<&str>,
    ) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference, shared_drive_id, shared_item_id, shared_kind,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0, NULL, NULL, NULL,
                    ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9,
                    'none', NULL, ?10
                )",
                params![
                    profile_id,
                    path,
                    remote_item_id,
                    bool_to_sql(remote_present),
                    bool_to_sql(local_present),
                    remote_size as i64,
                    local_size as i64,
                    remote_modified_ts,
                    local_modified_ts,
                    current_unix_seconds(),
                ],
            )
            .expect("insert sync_files row");
    }

    #[test]
    fn planner_derives_download_upload_and_conflict_actions() {
        let profile_id = test_profile_id("actions");
        clear_profile_rows(&profile_id);

        insert_sync_file_row(
            &profile_id,
            "remote-only.txt",
            true,
            false,
            100,
            0,
            40,
            0,
            Some("remote-only-id"),
        );
        insert_sync_file_row(
            &profile_id,
            "local-only.txt",
            false,
            true,
            0,
            120,
            0,
            50,
            None,
        );
        insert_sync_file_row(
            &profile_id,
            "remote-newer.txt",
            true,
            true,
            200,
            100,
            10,
            10,
            Some("remote-newer-id"),
        );
        insert_sync_file_row(
            &profile_id,
            "local-newer.txt",
            true,
            true,
            100,
            200,
            10,
            10,
            Some("local-newer-id"),
        );
        insert_sync_file_row(
            &profile_id,
            "size-conflict.txt",
            true,
            true,
            300,
            300,
            10,
            20,
            Some("size-conflict-id"),
        );

        let counters = recompute_sync_file_actions(&profile_id, true).expect("recompute planner actions");
        assert_eq!(counters.need_download_total, 2);
        assert_eq!(counters.need_upload_total, 2);
        assert_eq!(counters.conflict_total, 1);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let action_for_path = |path: &str| -> String {
            connection
                .query_row(
                    "SELECT desired_action FROM sync_files WHERE profile_id = ?1 AND path = ?2",
                    params![&profile_id, path],
                    |row| row.get::<_, String>(0),
                )
                .expect("read desired action")
        };

        assert_eq!(action_for_path("remote-only.txt"), PLANNER_ACTION_DOWNLOAD);
        assert_eq!(action_for_path("local-only.txt"), PLANNER_ACTION_UPLOAD);
        assert_eq!(action_for_path("remote-newer.txt"), PLANNER_ACTION_DOWNLOAD);
        assert_eq!(action_for_path("local-newer.txt"), PLANNER_ACTION_UPLOAD);
        assert_eq!(action_for_path("size-conflict.txt"), PLANNER_ACTION_CONFLICT);
    }

    #[test]
    fn planner_uses_none_for_local_newer_before_two_way_ready() {
        let profile_id = test_profile_id("pre-two-way");
        clear_profile_rows(&profile_id);

        insert_sync_file_row(
            &profile_id,
            "local-newer-before-two-way.txt",
            true,
            true,
            100,
            200,
            10,
            10,
            Some("local-before-id"),
        );

        let counters = recompute_sync_file_actions(&profile_id, false).expect("recompute planner actions");
        assert_eq!(counters.need_upload_total, 0);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let action: String = connection
            .query_row(
                "SELECT desired_action FROM sync_files WHERE profile_id = ?1 AND path = ?2",
                params![&profile_id, "local-newer-before-two-way.txt"],
                |row| row.get(0),
            )
            .expect("read desired action");
        assert_eq!(action, PLANNER_ACTION_NONE);
    }

    #[test]
    fn materialized_job_count_reports_active_download_and_upload_rows() {
        let profile_id = test_profile_id("materialized-counts");
        clear_profile_rows(&profile_id);
        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let now = current_unix_seconds();

        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, run_state, attempt_count, created_at, updated_at
                ) VALUES (?1, 'download', 'dl-id', 'dl.txt', 1, 1, 'queued', 'idle', 0, ?2, ?2)",
                params![&profile_id, now],
            )
            .expect("insert download job");
        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, run_state, attempt_count, created_at, updated_at
                ) VALUES (?1, 'upload', 'up-id', 'up.txt', 1, 1, 'retry_wait', 'idle', 0, ?2, ?2)",
                params![&profile_id, now],
            )
            .expect("insert upload job");

        let (downloads, uploads) = read_materialized_job_counts(&profile_id).expect("read job counts");
        assert_eq!(downloads, 1);
        assert_eq!(uploads, 1);
    }
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
