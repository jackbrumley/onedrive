use rusqlite::{params, Connection, OptionalExtension};

const DOWNLOAD_JOB_DIRECTION: &str = "download";
const UPLOAD_JOB_DIRECTION: &str = "upload";
const DOWNLOAD_JOB_STATE_QUEUED: &str = "queued";
const DOWNLOAD_JOB_STATE_IN_PROGRESS: &str = "in_progress";
const DOWNLOAD_JOB_STATE_RETRY_WAIT: &str = "retry_wait";
const DOWNLOAD_JOB_STATE_DONE: &str = "done";
const DOWNLOAD_JOB_STATE_FAILED_TERMINAL: &str = "failed_terminal";
const DOWNLOAD_JOB_STATE_SKIPPED: &str = "skipped";

#[derive(Debug, Clone)]
struct ClaimedDownloadJob {
    job_id: i64,
    item_id: String,
    path: String,
    remote_size: u64,
    remote_modified_ts: i64,
}

#[derive(Debug, Clone, Default)]
struct DownloadJobCounters {
    planned_total: usize,
    in_progress: usize,
    retry_waiting: usize,
    completed: usize,
    failed_terminal: usize,
    remaining: usize,
}

#[derive(Debug, Clone, Default)]
struct UploadJobCounters {
    planned_total: usize,
    in_progress: usize,
    completed: usize,
    failed_terminal: usize,
    remaining: usize,
}

#[derive(Debug, Clone, Default)]
struct SyncFilePlannerCounters {
    cloud_discovered_total: usize,
    local_discovered_total: usize,
    need_download_total: usize,
    need_upload_total: usize,
    conflict_total: usize,
}

fn sync_jobs_db_path(profile_id: &str) -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir
        .join("somedrive")
        .join("accounts")
        .join(profile_id)
        .join("sync_jobs.db"))
}

fn open_sync_jobs_connection(profile_id: &str) -> Result<Connection, String> {
    let db_path = sync_jobs_db_path(profile_id)?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed creating sync jobs directory '{}': {}",
                parent.display(),
                error
            )
        })?;
    }

    let connection = Connection::open(&db_path).map_err(|error| {
        format!(
            "Failed opening sync jobs DB '{}': {}",
            db_path.display(),
            error
        )
    })?;

    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS sync_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 profile_id TEXT NOT NULL,
                 direction TEXT NOT NULL,
                 item_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 remote_size INTEGER NOT NULL DEFAULT 0,
                 remote_modified_ts INTEGER NOT NULL DEFAULT 0,
                 state TEXT NOT NULL,
                 attempt_count INTEGER NOT NULL DEFAULT 0,
                 last_error TEXT,
                 next_retry_at INTEGER,
                 lease_owner TEXT,
                 lease_until INTEGER,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 started_at INTEGER,
                 finished_at INTEGER,
                 UNIQUE(profile_id, direction, item_id)
             );
             CREATE INDEX IF NOT EXISTS idx_sync_jobs_scheduler
                 ON sync_jobs(profile_id, direction, state, next_retry_at);
             CREATE INDEX IF NOT EXISTS idx_sync_jobs_leases
                 ON sync_jobs(profile_id, direction, lease_until);
             CREATE TABLE IF NOT EXISTS sync_files (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 profile_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 is_dir INTEGER NOT NULL DEFAULT 0,
                 remote_item_id TEXT,
                 remote_present INTEGER NOT NULL DEFAULT 0,
                 local_present INTEGER NOT NULL DEFAULT 0,
                 remote_size INTEGER NOT NULL DEFAULT 0,
                 local_size INTEGER NOT NULL DEFAULT 0,
                 remote_modified_ts INTEGER NOT NULL DEFAULT 0,
                 local_modified_ts INTEGER NOT NULL DEFAULT 0,
                 desired_action TEXT NOT NULL DEFAULT 'none',
                 conflict_state TEXT,
                 updated_at INTEGER NOT NULL,
                 UNIQUE(profile_id, path)
             );
             CREATE INDEX IF NOT EXISTS idx_sync_files_action
                 ON sync_files(profile_id, desired_action);
             CREATE INDEX IF NOT EXISTS idx_sync_files_remote_item
                 ON sync_files(profile_id, remote_item_id);",
        )
        .map_err(|error| format!("Failed initializing sync jobs schema: {error}"))?;

    Ok(connection)
}

fn reset_download_jobs(profile_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .execute(
            "DELETE FROM sync_jobs WHERE profile_id = ?1 AND direction = ?2",
            params![profile_id, DOWNLOAD_JOB_DIRECTION],
        )
        .map_err(|error| format!("Failed resetting download jobs: {error}"))?;
    Ok(())
}

fn upsert_download_job(
    profile_id: &str,
    item_id: &str,
    path: &str,
    remote_size: u64,
    remote_modified_ts: i64,
) -> Result<bool, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();

    let existing_id: Option<i64> = connection
        .query_row(
            "SELECT id FROM sync_jobs WHERE profile_id = ?1 AND direction = ?2 AND item_id = ?3",
            params![profile_id, DOWNLOAD_JOB_DIRECTION, item_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Failed querying existing download job: {error}"))?;

    if existing_id.is_none() {
        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, attempt_count, last_error, next_retry_at, lease_owner, lease_until,
                    created_at, updated_at, started_at, finished_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, 0, NULL, NULL, NULL, NULL,
                    ?8, ?8, NULL, NULL
                )",
                params![
                    profile_id,
                    DOWNLOAD_JOB_DIRECTION,
                    item_id,
                    path,
                    remote_size as i64,
                    remote_modified_ts,
                    DOWNLOAD_JOB_STATE_QUEUED,
                    now,
                ],
            )
            .map_err(|error| format!("Failed inserting download job: {error}"))?;
        return Ok(true);
    }

    connection
        .execute(
            "UPDATE sync_jobs
             SET path = ?1,
                 remote_size = ?2,
                 remote_modified_ts = ?3,
                 updated_at = ?4
             WHERE profile_id = ?5 AND direction = ?6 AND item_id = ?7",
            params![
                path,
                remote_size as i64,
                remote_modified_ts,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                item_id,
            ],
        )
        .map_err(|error| format!("Failed updating download job metadata: {error}"))?;
    Ok(false)
}

fn claim_download_jobs(
    profile_id: &str,
    lease_owner: &str,
    max_jobs: usize,
) -> Result<Vec<ClaimedDownloadJob>, String> {
    if max_jobs == 0 {
        return Ok(Vec::new());
    }

    let mut connection = open_sync_jobs_connection(profile_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed starting download claim transaction: {error}"))?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(120);

    transaction
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 next_retry_at = NULL,
                 updated_at = ?2
             WHERE profile_id = ?3
               AND direction = ?4
               AND state = ?5
               AND lease_until IS NOT NULL
               AND lease_until <= ?2",
            params![
                DOWNLOAD_JOB_STATE_QUEUED,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
            ],
        )
        .map_err(|error| format!("Failed recovering stale in-progress download jobs: {error}"))?;

    let mut statement = transaction
        .prepare(
            "SELECT id, item_id, path, remote_size, remote_modified_ts
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND state IN (?3, ?4)
               AND (next_retry_at IS NULL OR next_retry_at <= ?5)
             ORDER BY created_at ASC
             LIMIT ?6",
        )
        .map_err(|error| format!("Failed preparing download claim query: {error}"))?;

    let mut jobs: Vec<ClaimedDownloadJob> = Vec::new();
    let rows = statement
        .query_map(
            params![
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                DOWNLOAD_JOB_STATE_QUEUED,
                DOWNLOAD_JOB_STATE_RETRY_WAIT,
                now,
                max_jobs as i64,
            ],
            |row| {
                Ok(ClaimedDownloadJob {
                    job_id: row.get(0)?,
                    item_id: row.get(1)?,
                    path: row.get(2)?,
                    remote_size: row.get::<_, i64>(3)?.max(0) as u64,
                    remote_modified_ts: row.get(4)?,
                })
            },
        )
        .map_err(|error| format!("Failed executing download claim query: {error}"))?;

    for row in rows {
        jobs.push(row.map_err(|error| format!("Failed reading claimed download job: {error}"))?);
    }
    drop(statement);

    for job in &jobs {
        transaction
            .execute(
                "UPDATE sync_jobs
                 SET state = ?1,
                     attempt_count = attempt_count + 1,
                     lease_owner = ?2,
                     lease_until = ?3,
                     started_at = COALESCE(started_at, ?4),
                     updated_at = ?4,
                     last_error = NULL,
                     next_retry_at = NULL
                 WHERE id = ?5",
                params![
                    DOWNLOAD_JOB_STATE_IN_PROGRESS,
                    lease_owner,
                    lease_until,
                    now,
                    job.job_id,
                ],
            )
            .map_err(|error| format!("Failed marking download job as in-progress: {error}"))?;
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed committing download claim transaction: {error}"))?;
    Ok(jobs)
}

fn mark_download_job_done(profile_id: &str, job_id: i64, skipped: bool) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let final_state = if skipped {
        DOWNLOAD_JOB_STATE_SKIPPED
    } else {
        DOWNLOAD_JOB_STATE_DONE
    };
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 updated_at = ?2,
                 finished_at = ?2
             WHERE profile_id = ?3 AND direction = ?4 AND id = ?5",
            params![
                final_state,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking download job done: {error}"))?;
    Ok(())
}

fn mark_download_job_failed(profile_id: &str, job_id: i64, error_text: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?2,
                 updated_at = ?3,
                 finished_at = ?3
             WHERE profile_id = ?4 AND direction = ?5 AND id = ?6",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                error_text,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking download job failed: {error}"))?;
    Ok(())
}

fn read_download_job_counters(profile_id: &str) -> Result<DownloadJobCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT state, COUNT(1)
             FROM sync_jobs
             WHERE profile_id = ?1 AND direction = ?2
             GROUP BY state",
        )
        .map_err(|error| format!("Failed preparing download counters query: {error}"))?;

    let mut counters = DownloadJobCounters::default();
    let rows = statement
        .query_map(params![profile_id, DOWNLOAD_JOB_DIRECTION], |row| {
            let state: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((state, count.max(0) as usize))
        })
        .map_err(|error| format!("Failed querying download counters: {error}"))?;

    for row in rows {
        let (state, count) = row.map_err(|error| format!("Failed reading download counters: {error}"))?;
        counters.planned_total += count;
        match state.as_str() {
            DOWNLOAD_JOB_STATE_IN_PROGRESS => counters.in_progress = count,
            DOWNLOAD_JOB_STATE_RETRY_WAIT => counters.retry_waiting = count,
            DOWNLOAD_JOB_STATE_DONE => counters.completed = count,
            DOWNLOAD_JOB_STATE_FAILED_TERMINAL => counters.failed_terminal = count,
            DOWNLOAD_JOB_STATE_QUEUED => {}
            DOWNLOAD_JOB_STATE_SKIPPED => {}
            _ => {}
        }
    }

    counters.remaining = counters
        .planned_total
        .saturating_sub(counters.completed)
        .saturating_sub(counters.failed_terminal);
    Ok(counters)
}

fn begin_upload_job(
    profile_id: &str,
    relative_path: &str,
    size_bytes: u64,
    modified_ts: i64,
    lease_owner: &str,
) -> Result<i64, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(300);
    let item_id = relative_path;

    connection
        .execute(
            "INSERT INTO sync_jobs (
                profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                state, attempt_count, last_error, next_retry_at, lease_owner, lease_until,
                created_at, updated_at, started_at, finished_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, 1, NULL, NULL, ?8, ?9,
                ?10, ?10, ?10, NULL
            )
            ON CONFLICT(profile_id, direction, item_id)
            DO UPDATE SET
                path = excluded.path,
                remote_size = excluded.remote_size,
                remote_modified_ts = excluded.remote_modified_ts,
                state = excluded.state,
                attempt_count = sync_jobs.attempt_count + 1,
                last_error = NULL,
                next_retry_at = NULL,
                lease_owner = excluded.lease_owner,
                lease_until = excluded.lease_until,
                started_at = COALESCE(sync_jobs.started_at, excluded.started_at),
                finished_at = NULL,
                updated_at = excluded.updated_at",
            params![
                profile_id,
                UPLOAD_JOB_DIRECTION,
                item_id,
                relative_path,
                size_bytes as i64,
                modified_ts,
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                lease_owner,
                lease_until,
                now,
            ],
        )
        .map_err(|error| format!("Failed beginning upload job: {error}"))?;

    let job_id = connection
        .query_row(
            "SELECT id FROM sync_jobs WHERE profile_id = ?1 AND direction = ?2 AND item_id = ?3",
            params![profile_id, UPLOAD_JOB_DIRECTION, item_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed fetching upload job id: {error}"))?;
    Ok(job_id)
}

fn mark_upload_job_done(profile_id: &str, job_id: i64) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 updated_at = ?2,
                 finished_at = ?2
             WHERE profile_id = ?3 AND direction = ?4 AND id = ?5",
            params![
                DOWNLOAD_JOB_STATE_DONE,
                now,
                profile_id,
                UPLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking upload job done: {error}"))?;
    Ok(())
}

fn mark_upload_job_failed(profile_id: &str, job_id: i64, error_text: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?2,
                 updated_at = ?3,
                 finished_at = ?3
             WHERE profile_id = ?4 AND direction = ?5 AND id = ?6",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                error_text,
                now,
                profile_id,
                UPLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking upload job failed: {error}"))?;
    Ok(())
}

fn read_upload_job_counters(profile_id: &str) -> Result<UploadJobCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT state, COUNT(1)
             FROM sync_jobs
             WHERE profile_id = ?1 AND direction = ?2
             GROUP BY state",
        )
        .map_err(|error| format!("Failed preparing upload counters query: {error}"))?;

    let mut counters = UploadJobCounters::default();
    let rows = statement
        .query_map(params![profile_id, UPLOAD_JOB_DIRECTION], |row| {
            let state: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((state, count.max(0) as usize))
        })
        .map_err(|error| format!("Failed querying upload counters: {error}"))?;

    for row in rows {
        let (state, count) = row.map_err(|error| format!("Failed reading upload counters: {error}"))?;
        counters.planned_total += count;
        match state.as_str() {
            DOWNLOAD_JOB_STATE_IN_PROGRESS => counters.in_progress = count,
            DOWNLOAD_JOB_STATE_DONE => counters.completed = count,
            DOWNLOAD_JOB_STATE_FAILED_TERMINAL => counters.failed_terminal = count,
            DOWNLOAD_JOB_STATE_RETRY_WAIT => {}
            DOWNLOAD_JOB_STATE_QUEUED => {}
            DOWNLOAD_JOB_STATE_SKIPPED => {}
            _ => {}
        }
    }

    counters.remaining = counters
        .planned_total
        .saturating_sub(counters.completed)
        .saturating_sub(counters.failed_terminal);
    Ok(counters)
}

fn reset_sync_file_index(profile_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .execute(
            "DELETE FROM sync_files WHERE profile_id = ?1",
            params![profile_id],
        )
        .map_err(|error| format!("Failed resetting sync file index: {error}"))?;
    Ok(())
}

fn upsert_remote_sync_file(profile_id: &str, remote_item: &RemoteKnownItem) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "INSERT INTO sync_files (
                profile_id, path, is_dir, remote_item_id,
                remote_present, local_present,
                remote_size, local_size,
                remote_modified_ts, local_modified_ts,
                desired_action, conflict_state, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4,
                1, 0,
                ?5, 0,
                ?6, 0,
                'none', NULL, ?7
             )
             ON CONFLICT(profile_id, path)
             DO UPDATE SET
                is_dir = excluded.is_dir,
                remote_item_id = excluded.remote_item_id,
                remote_present = 1,
                remote_size = excluded.remote_size,
                remote_modified_ts = excluded.remote_modified_ts,
                updated_at = excluded.updated_at",
            params![
                profile_id,
                remote_item.path,
                if remote_item.is_dir { 1 } else { 0 },
                remote_item.id,
                remote_item.size as i64,
                remote_item.modified_ts,
                now,
            ],
        )
        .map_err(|error| format!("Failed upserting remote sync file row: {error}"))?;
    Ok(())
}

fn upsert_local_sync_file(
    profile_id: &str,
    relative_path: &str,
    local_entry: &LocalSnapshotEntry,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "INSERT INTO sync_files (
                profile_id, path, is_dir, remote_item_id,
                remote_present, local_present,
                remote_size, local_size,
                remote_modified_ts, local_modified_ts,
                desired_action, conflict_state, updated_at
             ) VALUES (
                ?1, ?2, ?3, NULL,
                0, 1,
                0, ?4,
                0, ?5,
                'none', NULL, ?6
             )
             ON CONFLICT(profile_id, path)
             DO UPDATE SET
                is_dir = excluded.is_dir,
                local_present = 1,
                local_size = excluded.local_size,
                local_modified_ts = excluded.local_modified_ts,
                updated_at = excluded.updated_at",
            params![
                profile_id,
                relative_path,
                if local_entry.is_dir { 1 } else { 0 },
                local_entry.size as i64,
                local_entry.modified_ts,
                now,
            ],
        )
        .map_err(|error| format!("Failed upserting local sync file row: {error}"))?;
    Ok(())
}

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
               AND remote_present = 1
               AND local_present = 0",
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
