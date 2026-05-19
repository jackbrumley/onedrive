fn upsert_action_job_queued(profile_id: &str, direction: &str, item_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "INSERT INTO sync_jobs (
                profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                state, run_state, attempt_count, last_error, next_retry_at, lease_owner, lease_until,
                bytes_done, bytes_total, progress_updated_at,
                created_at, updated_at, started_at, finished_at
            ) VALUES (
                ?1, ?2, ?3, ?3, 0, 0,
                ?4, ?5, 0, NULL, NULL, NULL, NULL,
                0, NULL, ?6,
                ?6, ?6, NULL, NULL
            )
            ON CONFLICT(profile_id, direction, item_id)
            DO UPDATE SET
                state = excluded.state,
                run_state = excluded.run_state,
                last_error = NULL,
                next_retry_at = NULL,
                lease_owner = NULL,
                lease_until = NULL,
                updated_at = excluded.updated_at,
                finished_at = NULL,
                progress_updated_at = excluded.progress_updated_at",
            params![
                profile_id,
                direction,
                item_id,
                DOWNLOAD_JOB_STATE_QUEUED,
                JOB_RUN_STATE_IDLE,
                now,
            ],
        )
        .map_err(|error| format!("Failed upserting action job: {error}"))?;
    Ok(())
}

fn claim_action_job_paths(
    profile_id: &str,
    direction: &str,
    lease_owner: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let mut connection = open_sync_jobs_connection(profile_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed starting action claim transaction: {error}"))?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(ACTION_JOB_LEASE_SECONDS);

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
                direction,
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
            ],
        )
        .map_err(|error| format!("Failed recovering stale in-progress action jobs: {error}"))?;

    let mut statement = transaction
        .prepare(
            "SELECT item_id
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND state IN (?3, ?4)
               AND (next_retry_at IS NULL OR next_retry_at <= ?5)
             ORDER BY created_at ASC",
        )
        .map_err(|error| format!("Failed preparing action claim query: {error}"))?;
    let rows = statement
        .query_map(
            params![
                profile_id,
                direction,
                DOWNLOAD_JOB_STATE_QUEUED,
                DOWNLOAD_JOB_STATE_RETRY_WAIT,
                now,
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Failed querying action claim rows: {error}"))?;

    let mut item_ids: Vec<String> = Vec::new();
    for row in rows {
        item_ids.push(row.map_err(|error| format!("Failed reading action claim row: {error}"))?);
    }
    drop(statement);

    for item_id in &item_ids {
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
                     progress_updated_at = ?5
                 WHERE profile_id = ?6
                   AND direction = ?7
                   AND item_id = ?8",
                params![
                    DOWNLOAD_JOB_STATE_IN_PROGRESS,
                    JOB_RUN_STATE_CLAIMED,
                    lease_owner,
                    lease_until,
                    now,
                    profile_id,
                    direction,
                    item_id,
                ],
            )
            .map_err(|error| format!("Failed claiming action job: {error}"))?;
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed committing action claim transaction: {error}"))?;

    Ok(item_ids.into_iter().collect())
}

fn mark_action_job_running(profile_id: &str, direction: &str, item_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    let lease_until = now.saturating_add(ACTION_JOB_LEASE_SECONDS);
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_until = ?3,
                 updated_at = ?4,
                 progress_updated_at = ?4
             WHERE profile_id = ?5 AND direction = ?6 AND item_id = ?7",
            params![
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
                JOB_RUN_STATE_RUNNING,
                lease_until,
                now,
                profile_id,
                direction,
                item_id,
            ],
        )
        .map_err(|error| format!("Failed marking action job running: {error}"))?;
    Ok(())
}

fn mark_action_job_failed(
    profile_id: &str,
    direction: &str,
    item_id: &str,
    error_text: &str,
) -> Result<(), String> {
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
             WHERE profile_id = ?5 AND direction = ?6 AND item_id = ?7",
            params![
                DOWNLOAD_JOB_STATE_FAILED_TERMINAL,
                JOB_RUN_STATE_IDLE,
                error_text,
                now,
                profile_id,
                direction,
                item_id,
            ],
        )
        .map_err(|error| format!("Failed marking action job failed: {error}"))?;
    Ok(())
}

fn mark_action_job_done(profile_id: &str, direction: &str, item_id: &str) -> Result<(), String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let now = current_unix_seconds();
    connection
        .execute(
            "UPDATE sync_jobs
             SET state = ?1,
                 run_state = ?2,
                 lease_owner = NULL,
                 lease_until = NULL,
                 updated_at = ?3,
                 finished_at = ?3,
                 progress_updated_at = ?3
             WHERE profile_id = ?4 AND direction = ?5 AND item_id = ?6",
            params![
                DOWNLOAD_JOB_STATE_DONE,
                JOB_RUN_STATE_IDLE,
                now,
                profile_id,
                direction,
                item_id,
            ],
        )
        .map_err(|error| format!("Failed marking action job done: {error}"))?;
    Ok(())
}

fn list_pending_action_job_paths(
    profile_id: &str,
    direction: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT item_id
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND state IN (?3, ?4, ?5)
             ORDER BY item_id ASC",
        )
        .map_err(|error| format!("Failed preparing action job path query: {error}"))?;
    let rows = statement
        .query_map(
            params![
                profile_id,
                direction,
                DOWNLOAD_JOB_STATE_QUEUED,
                DOWNLOAD_JOB_STATE_RETRY_WAIT,
                DOWNLOAD_JOB_STATE_IN_PROGRESS,
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Failed querying action job paths: {error}"))?;

    let mut item_ids = std::collections::HashSet::new();
    for row in rows {
        item_ids.insert(row.map_err(|error| format!("Failed reading action job path: {error}"))?);
    }
    Ok(item_ids)
}
