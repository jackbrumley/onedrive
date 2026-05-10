use crate::app::sync_runtime::{SyncRuntimeRecentItem, SyncRuntimeTransfer};
use chrono::{Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};

const DOWNLOAD_JOB_DIRECTION: &str = "download";
const UPLOAD_JOB_DIRECTION: &str = "upload";
const DOWNLOAD_JOB_STATE_QUEUED: &str = "queued";
const DOWNLOAD_JOB_STATE_IN_PROGRESS: &str = "in_progress";
const DOWNLOAD_JOB_STATE_RETRY_WAIT: &str = "retry_wait";
const DOWNLOAD_JOB_STATE_DONE: &str = "done";
const DOWNLOAD_JOB_STATE_FAILED_TERMINAL: &str = "failed_terminal";
const DOWNLOAD_JOB_STATE_SKIPPED: &str = "skipped";
const JOB_RUN_STATE_IDLE: &str = "idle";
const JOB_RUN_STATE_CLAIMED: &str = "claimed";
const JOB_RUN_STATE_RUNNING: &str = "running";
const DOWNLOAD_JOB_LEASE_SECONDS: i64 = 900;

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
    planned_bytes: u64,
    in_progress: usize,
    in_flight_bytes_done: u64,
    retry_waiting: usize,
    completed: usize,
    completed_bytes: u64,
    failed_terminal: usize,
    remaining: usize,
    remaining_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct UploadJobCounters {
    planned_total: usize,
    planned_bytes: u64,
    in_progress: usize,
    in_flight_bytes_done: u64,
    retry_waiting: usize,
    completed: usize,
    completed_bytes: u64,
    failed_terminal: usize,
    remaining_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct SyncJobActivityProjection {
    active: Vec<SyncRuntimeTransfer>,
    recent_completed: Vec<SyncRuntimeRecentItem>,
    recent_retry_waiting: Vec<SyncRuntimeRecentItem>,
    recent_failed: Vec<SyncRuntimeRecentItem>,
    active_download_count: usize,
    active_upload_count: usize,
}

#[derive(Debug, Clone, Default)]
struct SyncFilePlannerCounters {
    cloud_discovered_total: usize,
    local_discovered_total: usize,
    need_download_total: usize,
    need_upload_total: usize,
    conflict_total: usize,
}

#[derive(Debug, Clone)]
struct PersistedSyncIssue {
    issue_code: String,
    issue_message: String,
    issue_actions: Vec<String>,
    issue_path: Option<String>,
    issue_secondary_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ThrottleCounters {
    download_total: usize,
    download_last_minute: usize,
    upload_total: usize,
    upload_last_minute: usize,
}

#[derive(Debug, Clone)]
struct SyncLifecycleStateRow {
    two_way_ready: bool,
    bootstrap_scan_initialized: bool,
    bootstrap_full_scan_completed: bool,
    delta_link: Option<String>,
    active_delta_next_link: Option<String>,
    last_cycle_at: Option<String>,
    phase: String,
    phase_message: String,
    remote_scan_complete: bool,
    agent_state: String,
    last_sync_at: Option<String>,
}

impl Default for SyncLifecycleStateRow {
    fn default() -> Self {
        Self {
            two_way_ready: false,
            bootstrap_scan_initialized: false,
            bootstrap_full_scan_completed: false,
            delta_link: None,
            active_delta_next_link: None,
            last_cycle_at: None,
            phase: "idle".to_string(),
            phase_message: "Idle".to_string(),
            remote_scan_complete: false,
            agent_state: "idle".to_string(),
            last_sync_at: None,
        }
    }
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
             PRAGMA busy_timeout = 5000;
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
                 run_state TEXT NOT NULL DEFAULT 'idle',
                 attempt_count INTEGER NOT NULL DEFAULT 0,
                 last_error TEXT,
                 next_retry_at INTEGER,
                 lease_owner TEXT,
                 lease_until INTEGER,
                 bytes_done INTEGER NOT NULL DEFAULT 0,
                 bytes_total INTEGER,
                 progress_updated_at INTEGER,
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
             CREATE INDEX IF NOT EXISTS idx_sync_jobs_activity
                 ON sync_jobs(profile_id, direction, state, run_state, progress_updated_at);
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
                  ON sync_files(profile_id, remote_item_id);
             CREATE TABLE IF NOT EXISTS sync_issue_state (
                 profile_id TEXT PRIMARY KEY,
                 issue_code TEXT NOT NULL,
                 issue_message TEXT NOT NULL,
                 issue_actions_json TEXT NOT NULL,
                 issue_path TEXT,
                 issue_secondary_path TEXT,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sync_throttle_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 profile_id TEXT NOT NULL,
                 direction TEXT NOT NULL,
                 occurred_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sync_lifecycle_state (
                 profile_id TEXT PRIMARY KEY,
                 two_way_ready INTEGER NOT NULL DEFAULT 0,
                 bootstrap_scan_initialized INTEGER NOT NULL DEFAULT 0,
                 bootstrap_full_scan_completed INTEGER NOT NULL DEFAULT 0,
                 delta_link TEXT,
                 active_delta_next_link TEXT,
                 last_cycle_at TEXT,
                 phase TEXT NOT NULL DEFAULT 'idle',
                 phase_message TEXT NOT NULL DEFAULT 'Idle',
                 remote_scan_complete INTEGER NOT NULL DEFAULT 0,
                 agent_state TEXT NOT NULL DEFAULT 'idle',
                 last_sync_at TEXT,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sync_state_store (
                 profile_id TEXT PRIMARY KEY,
                 state_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_sync_throttle_events_lookup
                  ON sync_throttle_events(profile_id, direction, occurred_at)
             ;",
        )
        .map_err(|error| format!("Failed initializing sync jobs schema: {error}"))?;

    run_sync_jobs_migrations(&connection)?;

    Ok(connection)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryFailedDownloadJobStatus {
    Retried,
    AlreadyRetrying,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RetryAllFailedDownloadJobsReport {
    pub retried: usize,
    pub skipped_permission_denied: usize,
    pub already_retrying: usize,
}

fn run_sync_jobs_migrations(connection: &Connection) -> Result<(), String> {
    let current_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Failed reading sync_jobs schema version: {error}"))?;

    if current_version < 1 {
        add_sync_jobs_column_if_missing(
            connection,
            "run_state",
            "ALTER TABLE sync_jobs ADD COLUMN run_state TEXT NOT NULL DEFAULT 'idle'",
        )?;
        add_sync_jobs_column_if_missing(
            connection,
            "bytes_done",
            "ALTER TABLE sync_jobs ADD COLUMN bytes_done INTEGER NOT NULL DEFAULT 0",
        )?;
        add_sync_jobs_column_if_missing(
            connection,
            "bytes_total",
            "ALTER TABLE sync_jobs ADD COLUMN bytes_total INTEGER",
        )?;
        add_sync_jobs_column_if_missing(
            connection,
            "progress_updated_at",
            "ALTER TABLE sync_jobs ADD COLUMN progress_updated_at INTEGER",
        )?;
        connection
            .execute_batch(
                "UPDATE sync_jobs SET run_state = 'idle' WHERE run_state IS NULL;
                 UPDATE sync_jobs
                 SET run_state = 'idle'
                 WHERE state IN ('done', 'failed_terminal', 'retry_wait', 'queued', 'skipped');
                 UPDATE sync_jobs
                 SET run_state = 'claimed'
                 WHERE state = 'in_progress' AND run_state = 'idle';
                 UPDATE sync_jobs
                 SET progress_updated_at = COALESCE(progress_updated_at, updated_at)
                 WHERE progress_updated_at IS NULL;
                 PRAGMA user_version = 1;",
            )
            .map_err(|error| format!("Failed applying sync_jobs schema migration v1: {error}"))?;
    }

    let current_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Failed re-reading sync_jobs schema version: {error}"))?;
    if current_version < 2 {
        connection
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_sync_jobs_activity
                     ON sync_jobs(profile_id, direction, state, run_state, progress_updated_at);
                 PRAGMA user_version = 2;",
            )
            .map_err(|error| format!("Failed applying sync_jobs schema migration v2: {error}"))?;
    }

    Ok(())
}

fn bool_to_sql(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sql_to_bool(value: i64) -> bool {
    value != 0
}

fn upsert_sync_lifecycle_row(
    profile_id: &str,
    row: &SyncLifecycleStateRow,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "INSERT INTO sync_lifecycle_state (
                 profile_id,
                 two_way_ready,
                 bootstrap_scan_initialized,
                 bootstrap_full_scan_completed,
                 delta_link,
                 active_delta_next_link,
                 last_cycle_at,
                 phase,
                 phase_message,
                 remote_scan_complete,
                 agent_state,
                 last_sync_at,
                 updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(profile_id)
             DO UPDATE SET
                 two_way_ready = excluded.two_way_ready,
                 bootstrap_scan_initialized = excluded.bootstrap_scan_initialized,
                 bootstrap_full_scan_completed = excluded.bootstrap_full_scan_completed,
                 delta_link = excluded.delta_link,
                 active_delta_next_link = excluded.active_delta_next_link,
                 last_cycle_at = excluded.last_cycle_at,
                 phase = excluded.phase,
                 phase_message = excluded.phase_message,
                 remote_scan_complete = excluded.remote_scan_complete,
                 agent_state = excluded.agent_state,
                 last_sync_at = excluded.last_sync_at,
                 updated_at = excluded.updated_at",
            params![
                profile_id,
                bool_to_sql(row.two_way_ready),
                bool_to_sql(row.bootstrap_scan_initialized),
                bool_to_sql(row.bootstrap_full_scan_completed),
                row.delta_link,
                row.active_delta_next_link,
                row.last_cycle_at,
                row.phase,
                row.phase_message,
                bool_to_sql(row.remote_scan_complete),
                row.agent_state,
                row.last_sync_at,
                now,
            ],
        )
        .map_err(|error| format!("Failed upserting sync lifecycle row: {error}"))?;
    Ok(())
}

fn read_sync_lifecycle_row(profile_id: &str) -> Result<Option<SyncLifecycleStateRow>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .query_row(
            "SELECT two_way_ready,
                    bootstrap_scan_initialized,
                    bootstrap_full_scan_completed,
                    delta_link,
                    active_delta_next_link,
                    last_cycle_at,
                    phase,
                    phase_message,
                    remote_scan_complete,
                    agent_state,
                    last_sync_at
             FROM sync_lifecycle_state
             WHERE profile_id = ?1",
            params![profile_id],
            |row| {
                Ok(SyncLifecycleStateRow {
                    two_way_ready: sql_to_bool(row.get::<_, i64>(0)?),
                    bootstrap_scan_initialized: sql_to_bool(row.get::<_, i64>(1)?),
                    bootstrap_full_scan_completed: sql_to_bool(row.get::<_, i64>(2)?),
                    delta_link: row.get(3)?,
                    active_delta_next_link: row.get(4)?,
                    last_cycle_at: row.get(5)?,
                    phase: row.get(6)?,
                    phase_message: row.get(7)?,
                    remote_scan_complete: sql_to_bool(row.get::<_, i64>(8)?),
                    agent_state: row.get(9)?,
                    last_sync_at: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed reading sync lifecycle row: {error}"))
}

fn persist_sync_lifecycle_from_state(
    profile_id: &str,
    sync_state: &PersistedSyncState,
) -> Result<(), String> {
    let mut row = read_sync_lifecycle_row(profile_id)?.unwrap_or_default();
    row.two_way_ready = sync_state.two_way_ready;
    row.bootstrap_scan_initialized = sync_state.bootstrap_scan_initialized;
    row.bootstrap_full_scan_completed = sync_state.bootstrap_full_scan_completed;
    row.delta_link = sync_state.delta_link.clone();
    row.active_delta_next_link = sync_state.active_delta_next_link.clone();
    row.last_cycle_at = sync_state.last_cycle_at.clone();
    upsert_sync_lifecycle_row(profile_id, &row)
}

fn hydrate_sync_state_from_lifecycle(
    profile_id: &str,
    sync_state: &mut PersistedSyncState,
) -> Result<bool, String> {
    let Some(row) = read_sync_lifecycle_row(profile_id)? else {
        return Ok(false);
    };
    sync_state.two_way_ready = row.two_way_ready;
    sync_state.bootstrap_scan_initialized = row.bootstrap_scan_initialized;
    sync_state.bootstrap_full_scan_completed = row.bootstrap_full_scan_completed;
    sync_state.delta_link = row.delta_link;
    sync_state.active_delta_next_link = row.active_delta_next_link;
    sync_state.last_cycle_at = row.last_cycle_at;
    Ok(true)
}

fn persist_sync_lifecycle_phase(profile_id: &str, phase: &str, phase_message: &str) -> Result<(), String> {
    let mut row = read_sync_lifecycle_row(profile_id)?.unwrap_or_default();
    row.phase = phase.to_string();
    row.phase_message = phase_message.to_string();
    upsert_sync_lifecycle_row(profile_id, &row)
}

fn persist_sync_lifecycle_remote_scan_complete(profile_id: &str, complete: bool) -> Result<(), String> {
    let mut row = read_sync_lifecycle_row(profile_id)?.unwrap_or_default();
    row.remote_scan_complete = complete;
    upsert_sync_lifecycle_row(profile_id, &row)
}

pub(crate) fn persist_sync_lifecycle_agent_state(profile_id: &str, agent_state: &str) -> Result<(), String> {
    let mut row = read_sync_lifecycle_row(profile_id)?.unwrap_or_default();
    row.agent_state = agent_state.to_string();
    upsert_sync_lifecycle_row(profile_id, &row)
}

pub(crate) fn persist_sync_lifecycle_last_sync_at(
    profile_id: &str,
    last_sync_at: Option<&str>,
) -> Result<(), String> {
    let mut row = read_sync_lifecycle_row(profile_id)?.unwrap_or_default();
    row.last_sync_at = last_sync_at.map(ToString::to_string);
    upsert_sync_lifecycle_row(profile_id, &row)
}

pub(crate) fn read_sync_lifecycle_profile_metadata(
    profile_id: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    Ok(read_sync_lifecycle_row(profile_id)?.map(|row| (row.agent_state, row.last_sync_at)))
}

pub(crate) fn list_terminal_failed_download_paths(
    profile_id: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT path
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND state = ?3
             ORDER BY finished_at DESC, updated_at DESC
             LIMIT ?4",
        )
        .map_err(|error| format!("Failed preparing terminal failed path query: {error}"))?;
    let rows = statement
        .query_map(
            params![
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                limit as i64,
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Failed querying terminal failed download paths: {error}"))?;

    let mut paths: Vec<String> = Vec::new();
    for row in rows {
        paths.push(row.map_err(|error| format!("Failed reading terminal failed path row: {error}"))?);
    }
    Ok(paths)
}

pub(crate) fn read_sync_state_store(profile_id: &str) -> Result<Option<String>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .query_row(
            "SELECT state_json FROM sync_state_store WHERE profile_id = ?1",
            params![profile_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Failed reading sync state store row: {error}"))
}

pub(crate) fn write_sync_state_store(profile_id: &str, state_json: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "INSERT INTO sync_state_store (profile_id, state_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id)
             DO UPDATE SET
                 state_json = excluded.state_json,
                 updated_at = excluded.updated_at",
            params![profile_id, state_json, now],
        )
        .map_err(|error| format!("Failed writing sync state store row: {error}"))?;
    Ok(())
}

fn add_sync_jobs_column_if_missing(
    connection: &Connection,
    column_name: &str,
    alter_sql: &str,
) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sync_jobs)")
        .map_err(|error| format!("Failed preparing sync_jobs schema query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Failed querying sync_jobs schema info: {error}"))?;
    for row in rows {
        let existing = row.map_err(|error| format!("Failed reading sync_jobs schema row: {error}"))?;
        if existing == column_name {
            return Ok(());
        }
    }
    connection
        .execute(alter_sql, [])
        .map_err(|error| format!("Failed adding sync_jobs column '{column_name}': {error}"))?;
    Ok(())
}

fn remove_download_job_by_item_id(profile_id: &str, item_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .execute(
            "DELETE FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND item_id = ?3",
            params![profile_id, DOWNLOAD_JOB_DIRECTION, item_id],
        )
        .map_err(|error| format!("Failed removing download job by item id: {error}"))?;
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
                    state, run_state, attempt_count, last_error, next_retry_at, lease_owner, lease_until,
                    bytes_done, bytes_total, progress_updated_at,
                    created_at, updated_at, started_at, finished_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, 0, NULL, NULL, NULL, NULL,
                    0, NULL, ?9,
                    ?9, ?9, NULL, NULL
                )",
                params![
                    profile_id,
                    DOWNLOAD_JOB_DIRECTION,
                    item_id,
                    path,
                    remote_size as i64,
                    remote_modified_ts,
                    DOWNLOAD_JOB_STATE_QUEUED,
                    JOB_RUN_STATE_IDLE,
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
                 bytes_total = CASE WHEN ?2 > 0 THEN ?2 ELSE bytes_total END,
                 progress_updated_at = COALESCE(progress_updated_at, ?4),
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
    let lease_until = now.saturating_add(DOWNLOAD_JOB_LEASE_SECONDS);

    transaction
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 next_retry_at = NULL,
                 updated_at = ?3,
                 progress_updated_at = COALESCE(progress_updated_at, ?3)
             WHERE profile_id = ?4
               AND direction = ?5
               AND state = ?6
               AND lease_until IS NOT NULL
               AND lease_until <= ?3",
            params![
                DOWNLOAD_JOB_STATE_QUEUED,
                JOB_RUN_STATE_IDLE,
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
                     run_state = ?2,
                     attempt_count = attempt_count + 1,
                     lease_owner = ?3,
                     lease_until = ?4,
                     started_at = COALESCE(started_at, ?5),
                     updated_at = ?5,
                     last_error = NULL,
                     next_retry_at = NULL,
                     bytes_done = 0,
                     bytes_total = CASE WHEN remote_size > 0 THEN remote_size ELSE NULL END,
                     progress_updated_at = ?5
                 WHERE id = ?6",
                params![
                    DOWNLOAD_JOB_STATE_IN_PROGRESS,
                    JOB_RUN_STATE_CLAIMED,
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
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 bytes_done = COALESCE(bytes_total, bytes_done),
                 updated_at = ?3,
                 finished_at = ?3,
                 progress_updated_at = ?3
             WHERE profile_id = ?4 AND direction = ?5 AND id = ?6",
            params![
                final_state,
                JOB_RUN_STATE_IDLE,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking download job done: {error}"))?;
    Ok(())
}

fn mark_download_job_running(profile_id: &str, job_id: i64) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(DOWNLOAD_JOB_LEASE_SECONDS);
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 started_at = COALESCE(started_at, ?3),
                 lease_until = ?4,
                 updated_at = ?3,
                 progress_updated_at = ?3
              WHERE profile_id = ?5 AND direction = ?6 AND id = ?7",
            params![
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                JOB_RUN_STATE_RUNNING,
                now,
                lease_until,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking download job running: {error}"))?;
    Ok(())
}

fn update_download_job_progress(
    profile_id: &str,
    job_id: i64,
    bytes_done: u64,
    bytes_total: Option<u64>,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(DOWNLOAD_JOB_LEASE_SECONDS);
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 bytes_done = ?3,
                 bytes_total = COALESCE(?4, bytes_total),
                 lease_until = ?6,
                 updated_at = ?5,
                 progress_updated_at = ?5
             WHERE profile_id = ?7 AND direction = ?8 AND id = ?9",
            params![
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                JOB_RUN_STATE_RUNNING,
                bytes_done as i64,
                bytes_total.map(|value| value as i64),
                now,
                lease_until,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed updating download job progress: {error}"))?;
    Ok(())
}

fn mark_download_job_failed(profile_id: &str, job_id: i64, error_text: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?3,
                 updated_at = ?4,
                 finished_at = ?4,
                 progress_updated_at = ?4
             WHERE profile_id = ?5 AND direction = ?6 AND id = ?7",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                JOB_RUN_STATE_IDLE,
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

pub fn retry_failed_download_job(
    profile_id: &str,
    recent_item_id: &str,
) -> Result<RetryFailedDownloadJobStatus, String> {
    let job_id_text = recent_item_id
        .strip_prefix("job-")
        .ok_or_else(|| "Invalid sync activity item id; expected 'job-<id>'".to_string())?;
    let job_id = job_id_text
        .parse::<i64>()
        .map_err(|_| "Invalid sync activity job id".to_string())?;
    if job_id <= 0 {
        return Err("Invalid sync activity job id".to_string());
    }

    let connection = open_sync_jobs_connection(profile_id)?;
    let current_state: Option<String> = connection
        .query_row(
            "SELECT state
             FROM sync_jobs
             WHERE profile_id = ?1 AND direction = ?2 AND id = ?3",
            params![profile_id, DOWNLOAD_JOB_DIRECTION, job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Failed loading failed download job: {error}"))?;

    let Some(state) = current_state else {
        return Ok(RetryFailedDownloadJobStatus::AlreadyRetrying);
    };

    if state != DOWNLOAD_JOB_STATE_FAILED_TERMINAL {
        return Ok(RetryFailedDownloadJobStatus::AlreadyRetrying);
    }

    let now = current_unix_seconds();
    let updated = connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = NULL,
                 next_retry_at = NULL,
                 bytes_done = 0,
                 finished_at = NULL,
                 updated_at = ?3,
                 progress_updated_at = ?3
             WHERE profile_id = ?4
               AND direction = ?5
               AND id = ?6
               AND state = ?7",
            params![
                DOWNLOAD_JOB_STATE_QUEUED,
                JOB_RUN_STATE_IDLE,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
            ],
        )
        .map_err(|error| format!("Failed retrying failed download job: {error}"))?;

    if updated == 0 {
        return Ok(RetryFailedDownloadJobStatus::AlreadyRetrying);
    }

    Ok(RetryFailedDownloadJobStatus::Retried)
}

pub fn retry_all_failed_download_jobs(
    profile_id: &str,
) -> Result<RetryAllFailedDownloadJobsReport, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let failed_total: usize = connection
        .query_row(
            "SELECT
                SUM(CASE WHEN state = ?1 THEN 1 ELSE 0 END)
             FROM sync_jobs
             WHERE profile_id = ?2
               AND direction = ?3",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
            ],
            |row| Ok(row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as usize),
        )
        .map_err(|error| format!("Failed counting failed download jobs: {error}"))?;
    let now = current_unix_seconds();
    let retried = connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = NULL,
                 next_retry_at = NULL,
                 bytes_done = 0,
                 finished_at = NULL,
                 updated_at = ?3,
                 progress_updated_at = ?3
             WHERE profile_id = ?4
               AND direction = ?5
                AND state = ?6",
            params![
                DOWNLOAD_JOB_STATE_QUEUED,
                JOB_RUN_STATE_IDLE,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
            ],
        )
        .map_err(|error| format!("Failed retrying failed download jobs: {error}"))?
        as usize;

    let already_retrying = failed_total.saturating_sub(retried);

    Ok(RetryAllFailedDownloadJobsReport {
        retried,
        skipped_permission_denied: 0,
        already_retrying,
    })
}

fn mark_download_job_retry_wait(
    profile_id: &str,
    job_id: i64,
    error_text: &str,
    delay: Duration,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let delay_seconds = delay.as_secs().clamp(1, i64::MAX as u64) as i64;
    let next_retry_at = now.saturating_add(delay_seconds);
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?3,
                 next_retry_at = ?4,
                 updated_at = ?5,
                 finished_at = NULL,
                 progress_updated_at = ?5
             WHERE profile_id = ?6 AND direction = ?7 AND id = ?8",
            params![
                DOWNLOAD_JOB_STATE_RETRY_WAIT,
                JOB_RUN_STATE_IDLE,
                error_text,
                next_retry_at,
                now,
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking download job retry wait: {error}"))?;
    Ok(())
}

fn reset_running_sync_jobs_for_pause(profile_id: &str) -> Result<usize, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = CASE
                     WHEN state = ?1 THEN ?2
                     ELSE state
                 END,
                 run_state = ?3,
                 lease_owner = NULL,
                 lease_until = NULL,
                 next_retry_at = NULL,
                 updated_at = ?4,
                 progress_updated_at = COALESCE(progress_updated_at, ?4)
             WHERE profile_id = ?5
               AND run_state IN (?6, ?7)",
            params![
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                DOWNLOAD_JOB_STATE_QUEUED,
                JOB_RUN_STATE_IDLE,
                now,
                profile_id,
                JOB_RUN_STATE_RUNNING,
                JOB_RUN_STATE_CLAIMED,
            ],
        )
        .map_err(|error| format!("Failed resetting running sync jobs for pause: {error}"))
}

fn read_download_job_counters(profile_id: &str) -> Result<DownloadJobCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .query_row(
            "SELECT
                SUM(CASE WHEN state != 'skipped' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'in_progress' AND run_state = 'running' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'retry_wait' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'done' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'failed_terminal' THEN 1 ELSE 0 END),
                SUM(CASE
                    WHEN state IN ('queued', 'retry_wait') THEN COALESCE(bytes_total, remote_size, 0)
                    WHEN state = 'in_progress' AND run_state = 'running' THEN CASE
                        WHEN COALESCE(bytes_total, remote_size, 0) > COALESCE(bytes_done, 0)
                            THEN COALESCE(bytes_total, remote_size, 0) - COALESCE(bytes_done, 0)
                        ELSE 0
                    END
                    ELSE 0
                END),
                SUM(CASE WHEN state != 'skipped' THEN COALESCE(bytes_total, remote_size, 0) ELSE 0 END),
                SUM(CASE WHEN state = 'done' THEN COALESCE(bytes_total, bytes_done, remote_size, 0) ELSE 0 END),
                SUM(CASE WHEN state = 'in_progress' AND run_state = 'running' THEN COALESCE(bytes_done, 0) ELSE 0 END)
             FROM sync_jobs
             WHERE profile_id = ?1 AND direction = ?2",
            params![profile_id, DOWNLOAD_JOB_DIRECTION],
            |row| {
                let planned_total = row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as usize;
                let in_progress = row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as usize;
                let retry_waiting = row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as usize;
                let completed = row.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as usize;
                let failed_terminal = row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as usize;
                let remaining_bytes = row.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64;
                let planned_bytes = row.get::<_, Option<i64>>(6)?.unwrap_or(0).max(0) as u64;
                let completed_bytes = row.get::<_, Option<i64>>(7)?.unwrap_or(0).max(0) as u64;
                let in_flight_bytes_done = row.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as u64;
                let remaining = planned_total
                    .saturating_sub(completed)
                    .saturating_sub(failed_terminal);
                Ok(DownloadJobCounters {
                    planned_total,
                    planned_bytes,
                    in_progress,
                    in_flight_bytes_done,
                    retry_waiting,
                    completed,
                    completed_bytes,
                    failed_terminal,
                    remaining,
                    remaining_bytes,
                })
            },
        )
        .map_err(|error| format!("Failed reading download counters: {error}"))
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
                state, run_state, attempt_count, last_error, next_retry_at, lease_owner, lease_until,
                bytes_done, bytes_total, progress_updated_at,
                created_at, updated_at, started_at, finished_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, 1, NULL, NULL, ?9, ?10,
                0, ?11, ?11,
                ?11, ?11, ?11, NULL
            )
            ON CONFLICT(profile_id, direction, item_id)
            DO UPDATE SET
                path = excluded.path,
                remote_size = excluded.remote_size,
                remote_modified_ts = excluded.remote_modified_ts,
                state = excluded.state,
                run_state = excluded.run_state,
                attempt_count = sync_jobs.attempt_count + 1,
                last_error = NULL,
                next_retry_at = NULL,
                lease_owner = excluded.lease_owner,
                lease_until = excluded.lease_until,
                bytes_done = 0,
                bytes_total = excluded.bytes_total,
                progress_updated_at = excluded.progress_updated_at,
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
                JOB_RUN_STATE_RUNNING,
                lease_owner,
                lease_until,
                size_bytes as i64,
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
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 bytes_done = COALESCE(bytes_total, bytes_done),
                 updated_at = ?3,
                 finished_at = ?3,
                 progress_updated_at = ?3
             WHERE profile_id = ?4 AND direction = ?5 AND id = ?6",
            params![
                DOWNLOAD_JOB_STATE_DONE,
                JOB_RUN_STATE_IDLE,
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
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?3,
                 updated_at = ?4,
                 finished_at = ?4,
                 progress_updated_at = ?4
             WHERE profile_id = ?5 AND direction = ?6 AND id = ?7",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                JOB_RUN_STATE_IDLE,
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

fn mark_upload_job_retry_wait(
    profile_id: &str,
    job_id: i64,
    error_text: &str,
    delay: Duration,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let delay_seconds = delay.as_secs().clamp(1, i64::MAX as u64) as i64;
    let next_retry_at = now.saturating_add(delay_seconds);
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 last_error = ?3,
                 next_retry_at = ?4,
                 updated_at = ?5,
                 finished_at = NULL,
                 progress_updated_at = ?5
             WHERE profile_id = ?6 AND direction = ?7 AND id = ?8",
            params![
                DOWNLOAD_JOB_STATE_RETRY_WAIT,
                JOB_RUN_STATE_IDLE,
                error_text,
                next_retry_at,
                now,
                profile_id,
                UPLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed marking upload job retry wait: {error}"))?;
    Ok(())
}

fn update_upload_job_progress(
    profile_id: &str,
    job_id: i64,
    bytes_done: u64,
    bytes_total: Option<u64>,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 bytes_done = ?3,
                 bytes_total = COALESCE(?4, bytes_total),
                 updated_at = ?5,
                 progress_updated_at = ?5
             WHERE profile_id = ?6 AND direction = ?7 AND id = ?8",
            params![
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                JOB_RUN_STATE_RUNNING,
                bytes_done as i64,
                bytes_total.map(|value| value as i64),
                now,
                profile_id,
                UPLOAD_JOB_DIRECTION,
                job_id,
            ],
        )
        .map_err(|error| format!("Failed updating upload job progress: {error}"))?;
    Ok(())
}

fn read_upload_job_counters(profile_id: &str) -> Result<UploadJobCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .query_row(
            "SELECT
                SUM(CASE WHEN state != 'skipped' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'in_progress' AND run_state = 'running' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'retry_wait' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'done' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'failed_terminal' THEN 1 ELSE 0 END),
                SUM(CASE
                    WHEN state IN ('queued', 'retry_wait') THEN COALESCE(bytes_total, remote_size, 0)
                    WHEN state = 'in_progress' AND run_state = 'running' THEN CASE
                        WHEN COALESCE(bytes_total, remote_size, 0) > COALESCE(bytes_done, 0)
                            THEN COALESCE(bytes_total, remote_size, 0) - COALESCE(bytes_done, 0)
                        ELSE 0
                    END
                    ELSE 0
                END),
                SUM(CASE WHEN state != 'skipped' THEN COALESCE(bytes_total, remote_size, 0) ELSE 0 END),
                SUM(CASE WHEN state = 'done' THEN COALESCE(bytes_total, bytes_done, remote_size, 0) ELSE 0 END),
                SUM(CASE WHEN state = 'in_progress' AND run_state = 'running' THEN COALESCE(bytes_done, 0) ELSE 0 END)
             FROM sync_jobs
             WHERE profile_id = ?1 AND direction = ?2",
            params![profile_id, UPLOAD_JOB_DIRECTION],
            |row| {
                let planned_total = row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as usize;
                let in_progress = row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as usize;
                let retry_waiting = row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as usize;
                let completed = row.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as usize;
                let failed_terminal = row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as usize;
                let remaining_bytes = row.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64;
                let planned_bytes = row.get::<_, Option<i64>>(6)?.unwrap_or(0).max(0) as u64;
                let completed_bytes = row.get::<_, Option<i64>>(7)?.unwrap_or(0).max(0) as u64;
                let in_flight_bytes_done = row.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as u64;
                Ok(UploadJobCounters {
                    planned_total,
                    planned_bytes,
                    in_progress,
                    in_flight_bytes_done,
                    retry_waiting,
                    completed,
                    completed_bytes,
                    failed_terminal,
                    remaining_bytes,
                })
            },
        )
        .map_err(|error| format!("Failed reading upload counters: {error}"))
}

fn read_sync_job_activity_projection(
    profile_id: &str,
    active_limit: usize,
    recent_limit: usize,
) -> Result<SyncJobActivityProjection, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut projection = SyncJobActivityProjection::default();

    let mut active_statement = connection
        .prepare(
            "SELECT id,
                    state,
                    run_state,
                    direction,
                    path,
                    bytes_done,
                    bytes_total,
                    COALESCE(started_at, updated_at),
                    COALESCE(progress_updated_at, updated_at)
             FROM sync_jobs
             WHERE profile_id = ?1
                AND (
                    (state = ?2 AND run_state IN (?3, ?4))
                    OR state = ?5
                )
              ORDER BY COALESCE(progress_updated_at, updated_at) DESC
              LIMIT ?6",
        )
        .map_err(|error| format!("Failed preparing active sync job query: {error}"))?;
    let active_rows = active_statement
        .query_map(
            params![
                profile_id,
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                JOB_RUN_STATE_RUNNING,
                JOB_RUN_STATE_CLAIMED,
                DOWNLOAD_JOB_STATE_QUEUED,
                active_limit as i64,
            ],
            |row| {
                let job_id: i64 = row.get(0)?;
                let state: String = row.get(1)?;
                let run_state: String = row.get(2)?;
                let direction: String = row.get(3)?;
                let path: String = row.get(4)?;
                let bytes_done = row.get::<_, i64>(5)?.max(0) as u64;
                let bytes_total = row.get::<_, Option<i64>>(6)?.map(|value| value.max(0) as u64);
                let started_at_unix: i64 = row.get(7)?;
                let updated_at_unix: i64 = row.get(8)?;
                let transfer_state = if state == DOWNLOAD_JOB_STATE_IN_PROGRESS
                    && run_state == JOB_RUN_STATE_RUNNING
                {
                    "in_progress"
                } else {
                    "queued"
                };
                Ok((
                    SyncRuntimeTransfer {
                        id: format!("job-{job_id}"),
                        direction,
                        path,
                        state: transfer_state.to_string(),
                        bytes_done,
                        bytes_total,
                        started_at: unix_seconds_to_rfc3339(started_at_unix),
                        updated_at: unix_seconds_to_rfc3339(updated_at_unix),
                    },
                    job_id,
                ))
            },
        )
        .map_err(|error| format!("Failed querying active sync jobs: {error}"))?;
    for row in active_rows {
        let (transfer, _) =
            row.map_err(|error| format!("Failed reading active sync job row: {error}"))?;
        if transfer.direction.eq_ignore_ascii_case(DOWNLOAD_JOB_DIRECTION)
            && transfer.state == "in_progress"
        {
            projection.active_download_count += 1;
        } else if transfer.direction.eq_ignore_ascii_case(UPLOAD_JOB_DIRECTION)
            && transfer.state == "in_progress"
        {
            projection.active_upload_count += 1;
        }
        projection.active.push(transfer);
    }

    let mut completed_statement = connection
        .prepare(
            "SELECT id,
                    direction,
                    path,
                    bytes_total,
                    finished_at
             FROM sync_jobs
             WHERE profile_id = ?1
               AND state IN (?2, ?3)
               AND finished_at IS NOT NULL
             ORDER BY finished_at DESC
             LIMIT ?4",
        )
        .map_err(|error| format!("Failed preparing completed sync job query: {error}"))?;
    let completed_rows = completed_statement
        .query_map(
            params![
                profile_id,
                DOWNLOAD_JOB_STATE_DONE,
                DOWNLOAD_JOB_STATE_SKIPPED,
                recent_limit as i64,
            ],
            |row| {
                let job_id: i64 = row.get(0)?;
                let direction: String = row.get(1)?;
                let path: String = row.get(2)?;
                let bytes_total = row.get::<_, Option<i64>>(3)?.map(|value| value.max(0) as u64);
                let finished_at_unix: i64 = row.get(4)?;
                Ok(SyncRuntimeRecentItem {
                    id: format!("job-{job_id}"),
                    direction,
                    path,
                    bytes_total,
                    finished_at: unix_seconds_to_rfc3339(finished_at_unix),
                    status: "completed".to_string(),
                    error: None,
                })
            },
        )
        .map_err(|error| format!("Failed querying completed sync jobs: {error}"))?;
    for row in completed_rows {
        projection.recent_completed.push(
            row.map_err(|error| format!("Failed reading completed sync job row: {error}"))?,
        );
    }

    let mut failed_statement = connection
        .prepare(
            "SELECT id,
                    direction,
                    path,
                    bytes_total,
                    finished_at,
                    last_error
             FROM sync_jobs
             WHERE profile_id = ?1
               AND state = ?2
               AND finished_at IS NOT NULL
             ORDER BY finished_at DESC
             LIMIT ?3",
        )
        .map_err(|error| format!("Failed preparing failed sync job query: {error}"))?;
    let failed_rows = failed_statement
        .query_map(
            params![profile_id, DOWNLOAD_JOB_STATE_FAILED_TERMINAL, recent_limit as i64],
            |row| {
                let job_id: i64 = row.get(0)?;
                let direction: String = row.get(1)?;
                let path: String = row.get(2)?;
                let bytes_total = row.get::<_, Option<i64>>(3)?.map(|value| value.max(0) as u64);
                let finished_at_unix: i64 = row.get(4)?;
                let error_text: Option<String> = row.get(5)?;
                Ok(SyncRuntimeRecentItem {
                    id: format!("job-{job_id}"),
                    direction,
                    path,
                    bytes_total,
                    finished_at: unix_seconds_to_rfc3339(finished_at_unix),
                    status: "failed".to_string(),
                    error: error_text,
                })
            },
        )
        .map_err(|error| format!("Failed querying failed sync jobs: {error}"))?;
    for row in failed_rows {
        projection
            .recent_failed
            .push(row.map_err(|error| format!("Failed reading failed sync job row: {error}"))?);
    }

    let mut retry_wait_statement = connection
        .prepare(
            "SELECT id,
                    direction,
                    path,
                    bytes_total,
                    COALESCE(next_retry_at, updated_at),
                    last_error
             FROM sync_jobs
             WHERE profile_id = ?1
               AND state = ?2
             ORDER BY COALESCE(next_retry_at, updated_at) ASC
             LIMIT ?3",
        )
        .map_err(|error| format!("Failed preparing retry-wait sync job query: {error}"))?;
    let retry_wait_rows = retry_wait_statement
        .query_map(
            params![profile_id, DOWNLOAD_JOB_STATE_RETRY_WAIT, recent_limit as i64],
            |row| {
                let job_id: i64 = row.get(0)?;
                let direction: String = row.get(1)?;
                let path: String = row.get(2)?;
                let bytes_total = row.get::<_, Option<i64>>(3)?.map(|value| value.max(0) as u64);
                let retry_at_unix: i64 = row.get(4)?;
                let error_text: Option<String> = row.get(5)?;
                Ok(SyncRuntimeRecentItem {
                    id: format!("job-{job_id}"),
                    direction,
                    path,
                    bytes_total,
                    finished_at: unix_seconds_to_rfc3339(retry_at_unix),
                    status: "retry_waiting".to_string(),
                    error: error_text,
                })
            },
        )
        .map_err(|error| format!("Failed querying retry-wait sync jobs: {error}"))?;
    for row in retry_wait_rows {
        projection.recent_retry_waiting.push(
            row.map_err(|error| format!("Failed reading retry-wait sync job row: {error}"))?,
        );
    }

    Ok(projection)
}

pub fn hydrate_runtime_status_from_db(
    status: &mut sync_runtime::SyncRuntimeAccountStatus,
) -> Result<(), String> {
    let profile_id = status.profile_id.clone();
    let projection = read_sync_job_activity_projection(&profile_id, 64, 120)?;
    let download_counters = read_download_job_counters(&profile_id)?;
    let upload_counters = read_upload_job_counters(&profile_id)?;

    if projection.active_download_count != download_counters.in_progress {
        log::warn!(
            "{} SYNC_ACTIVITY_INVARIANT_MISMATCH lane=download running_rows={} counter_in_flight={}",
            log_context::account_prefix(&profile_id),
            projection.active_download_count,
            download_counters.in_progress
        );
    }
    if projection.active_upload_count != upload_counters.in_progress {
        log::warn!(
            "{} SYNC_ACTIVITY_INVARIANT_MISMATCH lane=upload running_rows={} counter_in_flight={}",
            log_context::account_prefix(&profile_id),
            projection.active_upload_count,
            upload_counters.in_progress
        );
    }

    status.in_progress = projection.active;
    status.recent_completed = projection.recent_completed;
    status.recent_retry_waiting = projection.recent_retry_waiting;
    status.recent_failed = projection.recent_failed;

    status.remote_download_planned_total = download_counters.planned_total;
    status.remote_download_completed_total = download_counters.completed;
    status.remote_download_failed_total = download_counters.failed_terminal;
    status.remote_download_in_flight = download_counters.in_progress;
    status.remote_download_retry_waiting = download_counters.retry_waiting;
    status.remote_download_planned_bytes_total = download_counters.planned_bytes;
    status.remote_download_completed_bytes_total = download_counters.completed_bytes;
    status.remote_download_remaining_bytes_total = download_counters.remaining_bytes;
    status.remote_download_in_flight_bytes_done = download_counters.in_flight_bytes_done;
    status.remote_download_queue_count = status.remote_download_in_flight;
    status.remote_downloaded_count = status.remote_download_completed_total;

    status.upload_planned_total = upload_counters.planned_total;
    status.upload_completed_total = upload_counters.completed;
    status.upload_failed_total = upload_counters.failed_terminal;
    status.upload_in_flight = upload_counters.in_progress;
    status.upload_retry_waiting = upload_counters.retry_waiting;
    status.upload_planned_bytes_total = upload_counters.planned_bytes;
    status.upload_completed_bytes_total = upload_counters.completed_bytes;
    status.upload_remaining_bytes_total = upload_counters.remaining_bytes;
    status.upload_in_flight_bytes_done = upload_counters.in_flight_bytes_done;

    let throttle = read_throttle_counters(&profile_id)?;
    status.remote_download_throttle_total = throttle.download_total;
    status.remote_download_throttle_last_minute = throttle.download_last_minute;
    status.upload_throttle_total = throttle.upload_total;
    status.upload_throttle_last_minute = throttle.upload_last_minute;

    if let Some(issue) = read_persisted_sync_issue(&profile_id)? {
        status.issue_code = Some(issue.issue_code);
        status.issue_message = Some(issue.issue_message);
        status.issue_actions = issue.issue_actions;
        status.issue_path = issue.issue_path;
        status.issue_secondary_path = issue.issue_secondary_path;
    } else {
        status.issue_code = None;
        status.issue_message = None;
        status.issue_actions.clear();
        status.issue_path = None;
        status.issue_secondary_path = None;
    }

    if let Some(lifecycle) = read_sync_lifecycle_row(&profile_id)? {
        status.phase = lifecycle.phase;
        status.phase_message = lifecycle.phase_message;
        status.remote_scan_complete = lifecycle.remote_scan_complete;
        status.two_way_ready = lifecycle.two_way_ready;
    }

    Ok(())
}

fn persist_sync_issue(
    profile_id: &str,
    issue_code: &str,
    issue_message: &str,
    issue_actions: &[&str],
    issue_path: Option<&str>,
    issue_secondary_path: Option<&str>,
) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let issue_actions_json = serde_json::to_string(issue_actions)
        .map_err(|error| format!("Failed serializing issue actions: {error}"))?;
    connection
        .execute(
            "INSERT INTO sync_issue_state (
                profile_id,
                issue_code,
                issue_message,
                issue_actions_json,
                issue_path,
                issue_secondary_path,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(profile_id)
             DO UPDATE SET
                issue_code = excluded.issue_code,
                issue_message = excluded.issue_message,
                issue_actions_json = excluded.issue_actions_json,
                issue_path = excluded.issue_path,
                issue_secondary_path = excluded.issue_secondary_path,
                updated_at = excluded.updated_at",
            params![
                profile_id,
                issue_code,
                issue_message,
                issue_actions_json,
                issue_path,
                issue_secondary_path,
                now,
            ],
        )
        .map_err(|error| format!("Failed persisting sync issue: {error}"))?;
    Ok(())
}

fn clear_persisted_sync_issue(profile_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    connection
        .execute(
            "DELETE FROM sync_issue_state WHERE profile_id = ?1",
            params![profile_id],
        )
        .map_err(|error| format!("Failed clearing sync issue state: {error}"))?;
    Ok(())
}

fn read_persisted_sync_issue(profile_id: &str) -> Result<Option<PersistedSyncIssue>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let row = connection
        .query_row(
            "SELECT issue_code, issue_message, issue_actions_json, issue_path, issue_secondary_path
             FROM sync_issue_state
             WHERE profile_id = ?1",
            params![profile_id],
            |row| {
                let issue_actions_json: String = row.get(2)?;
                let issue_actions = serde_json::from_str::<Vec<String>>(&issue_actions_json)
                    .unwrap_or_default();
                Ok(PersistedSyncIssue {
                    issue_code: row.get(0)?,
                    issue_message: row.get(1)?,
                    issue_actions,
                    issue_path: row.get(3)?,
                    issue_secondary_path: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed reading sync issue state: {error}"))?;
    Ok(row)
}

fn record_throttle_event(profile_id: &str, direction: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let trim_before = now.saturating_sub(3600);
    connection
        .execute(
            "INSERT INTO sync_throttle_events (profile_id, direction, occurred_at) VALUES (?1, ?2, ?3)",
            params![profile_id, direction, now],
        )
        .map_err(|error| format!("Failed recording throttle event: {error}"))?;
    connection
        .execute(
            "DELETE FROM sync_throttle_events WHERE profile_id = ?1 AND occurred_at < ?2",
            params![profile_id, trim_before],
        )
        .map_err(|error| format!("Failed trimming throttle events: {error}"))?;
    Ok(())
}

fn read_throttle_counters(profile_id: &str) -> Result<ThrottleCounters, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let last_minute = now.saturating_sub(60);
    connection
        .query_row(
            "SELECT
                SUM(CASE WHEN direction = ?2 THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction = ?2 AND occurred_at >= ?4 THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction = ?3 THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction = ?3 AND occurred_at >= ?4 THEN 1 ELSE 0 END)
             FROM sync_throttle_events
             WHERE profile_id = ?1",
            params![
                profile_id,
                DOWNLOAD_JOB_DIRECTION,
                UPLOAD_JOB_DIRECTION,
                last_minute,
            ],
            |row| {
                Ok(ThrottleCounters {
                    download_total: row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as usize,
                    download_last_minute: row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0)
                        as usize,
                    upload_total: row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as usize,
                    upload_last_minute: row.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as usize,
                })
            },
        )
        .map_err(|error| format!("Failed reading throttle counters: {error}"))
}

fn unix_seconds_to_rfc3339(value: i64) -> String {
    Local
        .timestamp_opt(value, 0)
        .single()
        .map(|instant| instant.to_rfc3339())
        .unwrap_or_else(|| Local::now().to_rfc3339())
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
