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
            .map_err(|error| {
                format!("Failed deriving upload actions for local-only files: {error}")
            })?;
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
