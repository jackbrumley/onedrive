#[cfg(test)]
mod job_queue_tests {
    use super::*;

    fn test_profile_id(label: &str) -> String {
        format!(
            "job-queue-test-{label}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    }

    fn clear_profile_rows(profile_id: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute("DELETE FROM sync_jobs WHERE profile_id = ?1", params![profile_id])
            .expect("clear sync_jobs");
        connection
            .execute("DELETE FROM sync_files WHERE profile_id = ?1", params![profile_id])
            .expect("clear sync_files");
        connection
            .execute(
                "DELETE FROM sync_lifecycle_state WHERE profile_id = ?1",
                params![profile_id],
            )
            .expect("clear sync_lifecycle_state");
    }

    fn insert_running_job(profile_id: &str, direction: &str, item_id: &str, path: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        let now = current_unix_seconds();
        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, run_state, attempt_count, last_error, next_retry_at,
                    lease_owner, lease_until, bytes_done, bytes_total, progress_updated_at,
                    created_at, updated_at, started_at, finished_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, 1, 1,
                    'in_progress', 'running', 1, NULL, NULL,
                    'test-lease', ?5, 0, 1, ?5,
                    ?5, ?5, ?5, NULL
                )",
                params![profile_id, direction, item_id, path, now],
            )
            .expect("insert running sync job");
    }

    #[test]
    fn reset_running_jobs_for_pause_requeues_all_directions() {
        let profile_id = test_profile_id("pause-reset");
        clear_profile_rows(&profile_id);
        insert_running_job(&profile_id, DOWNLOAD_JOB_DIRECTION, "dl-1", "download.txt");
        insert_running_job(&profile_id, UPLOAD_JOB_DIRECTION, "up-1", "upload.txt");

        let reset_count = reset_running_sync_jobs_for_pause(&profile_id).expect("reset running jobs");
        assert_eq!(reset_count, 2);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let mut statement = connection
            .prepare(
                "SELECT direction, state, run_state
                 FROM sync_jobs
                 WHERE profile_id = ?1
                 ORDER BY direction ASC",
            )
            .expect("prepare job state query");
        let rows = statement
            .query_map(params![&profile_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .expect("query job state rows");

        let collected: Vec<(String, String, String)> =
            rows.map(|row| row.expect("read job state row")).collect();
        assert_eq!(collected.len(), 2);
        for (_direction, state, run_state) in collected {
            assert_eq!(state, DOWNLOAD_JOB_STATE_QUEUED);
            assert_eq!(run_state, JOB_RUN_STATE_IDLE);
        }
    }

    #[test]
    fn read_sync_authority_row_counts_reports_table_counts() {
        let profile_id = test_profile_id("row-counts");
        clear_profile_rows(&profile_id);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let now = current_unix_seconds();
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, 'file.txt', 0, 0,
                    'remote-id', 1, 1,
                    10, 10, 10, 10,
                    'none', NULL, ?2
                )",
                params![&profile_id, now],
            )
            .expect("insert sync_files row");
        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, run_state, attempt_count, created_at, updated_at
                ) VALUES (?1, 'download', 'job-id', 'file.txt', 10, 10, 'queued', 'idle', 0, ?2, ?2)",
                params![&profile_id, now],
            )
            .expect("insert sync_jobs row");
        persist_sync_lifecycle_phase(&profile_id, "idle", "Idle").expect("persist lifecycle row");

        let (lifecycle_rows, planner_rows, job_rows) =
            read_sync_authority_row_counts(&profile_id).expect("read authority row counts");
        assert_eq!(lifecycle_rows, 1);
        assert_eq!(planner_rows, 1);
        assert_eq!(job_rows, 1);
    }

    #[test]
    fn claim_action_job_paths_marks_jobs_claimed() {
        let profile_id = test_profile_id("claim-action-jobs");
        clear_profile_rows(&profile_id);

        upsert_action_job_queued(
            &profile_id,
            DELETE_LOCAL_JOB_DIRECTION,
            "docs/delete-local.txt",
        )
        .expect("queue action job");
        let claimed = claim_action_job_paths(&profile_id, DELETE_LOCAL_JOB_DIRECTION, "cycle-test")
            .expect("claim action job paths");

        assert!(claimed.contains("docs/delete-local.txt"));

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let (state, run_state): (String, String) = connection
            .query_row(
                "SELECT state, run_state
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = ?3",
                params![
                    &profile_id,
                    DELETE_LOCAL_JOB_DIRECTION,
                    "docs/delete-local.txt"
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read claimed action job state");
        assert_eq!(state, DOWNLOAD_JOB_STATE_IN_PROGRESS);
        assert_eq!(run_state, JOB_RUN_STATE_CLAIMED);
    }

    #[test]
    fn claim_upload_job_path_transitions_to_claimed_and_running() {
        let profile_id = test_profile_id("claim-upload-job");
        clear_profile_rows(&profile_id);

        upsert_upload_job_queued(&profile_id, "docs/upload.txt", 42, 100).expect("queue upload job");
        let claimed = claim_upload_job_path(&profile_id, "docs/upload.txt", "cycle-test")
            .expect("claim upload job path");
        assert!(claimed);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let (claimed_state, claimed_run_state): (String, String) = connection
            .query_row(
                "SELECT state, run_state
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = ?3",
                params![&profile_id, UPLOAD_JOB_DIRECTION, "docs/upload.txt"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read claimed upload job state");
        assert_eq!(claimed_state, DOWNLOAD_JOB_STATE_IN_PROGRESS);
        assert_eq!(claimed_run_state, JOB_RUN_STATE_CLAIMED);

        let job_id = begin_upload_job(&profile_id, "docs/upload.txt", 42, 100, "cycle-test")
            .expect("begin upload job for claimed path");
        assert!(job_id > 0);

        let (running_state, running_run_state): (String, String) = connection
            .query_row(
                "SELECT state, run_state
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = ?3",
                params![&profile_id, UPLOAD_JOB_DIRECTION, "docs/upload.txt"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read running upload job state");
        assert_eq!(running_state, DOWNLOAD_JOB_STATE_IN_PROGRESS);
        assert_eq!(running_run_state, JOB_RUN_STATE_RUNNING);
    }

    #[test]
    fn lifecycle_operational_state_round_trips_through_db() {
        let profile_id = test_profile_id("lifecycle-operational");
        clear_profile_rows(&profile_id);

        let expected = SyncLifecycleOperationalState {
            two_way_ready: true,
            bootstrap_scan_initialized: true,
            bootstrap_full_scan_completed: true,
            delta_link: Some("https://example.test/delta".to_string()),
            active_delta_next_link: Some("https://example.test/next".to_string()),
            last_cycle_at: Some("2026-01-01T00:00:00+00:00".to_string()),
        };

        persist_sync_lifecycle_operational_state(&profile_id, &expected)
            .expect("persist lifecycle operational state");
        let actual = read_sync_lifecycle_operational_state(&profile_id)
            .expect("read lifecycle operational state");

        assert_eq!(actual.two_way_ready, expected.two_way_ready);
        assert_eq!(
            actual.bootstrap_scan_initialized,
            expected.bootstrap_scan_initialized
        );
        assert_eq!(
            actual.bootstrap_full_scan_completed,
            expected.bootstrap_full_scan_completed
        );
        assert_eq!(actual.delta_link, expected.delta_link);
        assert_eq!(
            actual.active_delta_next_link,
            expected.active_delta_next_link
        );
        assert_eq!(actual.last_cycle_at, expected.last_cycle_at);
    }
}
