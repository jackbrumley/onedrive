struct PlannerMaterializationSummary {
    upload_paths: std::collections::HashSet<String>,
    delete_remote_paths: std::collections::HashSet<String>,
    delete_local_paths: std::collections::HashSet<String>,
    conflict_paths: Vec<String>,
    desired_download_paths: usize,
    desired_upload_paths: usize,
    desired_delete_remote_paths: usize,
    desired_delete_local_paths: usize,
    desired_conflict_paths: usize,
    active_download_jobs: usize,
    active_upload_jobs: usize,
}

fn materialize_planner_actions(
    profile_id: &str,
    account_prefix: &str,
    cycle_id: &str,
) -> Result<PlannerMaterializationSummary, String> {
    let download_candidates = list_sync_file_download_candidates(profile_id)?;
    let upload_candidates = list_sync_file_upload_candidates(profile_id)?;

    let mut desired_download_item_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for candidate in &download_candidates {
        desired_download_item_ids.insert(candidate.item_id.clone());
        let _ = upsert_download_job(
            profile_id,
            &candidate.item_id,
            &candidate.path,
            candidate.remote_size,
            candidate.remote_modified_ts,
        )?;
    }

    let mut upload_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut desired_upload_item_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for candidate in &upload_candidates {
        upload_paths.insert(candidate.path.clone());
        desired_upload_item_ids.insert(candidate.path.clone());
        let _ = upsert_upload_job_queued(
            profile_id,
            &candidate.path,
            candidate.local_size,
            candidate.local_modified_ts,
        )?;
    }

    prune_unplanned_queued_jobs(profile_id, DOWNLOAD_JOB_DIRECTION, &desired_download_item_ids)?;
    prune_unplanned_queued_jobs(profile_id, UPLOAD_JOB_DIRECTION, &desired_upload_item_ids)?;

    let desired_delete_remote_paths: std::collections::HashSet<String> =
        list_sync_file_paths_by_desired_action(profile_id, PLANNER_ACTION_DELETE_REMOTE)?
            .into_iter()
            .collect();
    let desired_delete_local_paths: std::collections::HashSet<String> =
        list_sync_file_paths_by_desired_action(profile_id, PLANNER_ACTION_DELETE_LOCAL)?
            .into_iter()
            .collect();
    let desired_conflict_paths: std::collections::HashSet<String> =
        list_sync_file_paths_by_desired_action(profile_id, PLANNER_ACTION_CONFLICT)?
            .into_iter()
            .collect();

    materialize_action_jobs(
        profile_id,
        DELETE_REMOTE_JOB_DIRECTION,
        &desired_delete_remote_paths,
    )?;
    materialize_action_jobs(
        profile_id,
        DELETE_LOCAL_JOB_DIRECTION,
        &desired_delete_local_paths,
    )?;
    materialize_action_jobs(profile_id, CONFLICT_JOB_DIRECTION, &desired_conflict_paths)?;

    let delete_remote_paths = list_pending_action_job_paths(profile_id, DELETE_REMOTE_JOB_DIRECTION)?;
    let delete_local_paths = list_pending_action_job_paths(profile_id, DELETE_LOCAL_JOB_DIRECTION)?;
    let conflict_paths_set = list_pending_action_job_paths(profile_id, CONFLICT_JOB_DIRECTION)?;
    let mut conflict_paths: Vec<String> = conflict_paths_set.into_iter().collect();
    conflict_paths.sort();

    let desired_delete_remote_paths_count = desired_delete_remote_paths.len();
    let desired_delete_local_paths_count = desired_delete_local_paths.len();
    let desired_conflict_paths_count = desired_conflict_paths.len();

    let (active_download_jobs, active_upload_jobs) = read_materialized_job_counts(profile_id)?;
    log::info!(
        "{} [cycle:{}] PLANNER_MATERIALIZATION_SUMMARY desired_download_paths={} desired_upload_paths={} desired_delete_remote_paths={} desired_delete_local_paths={} desired_conflict_paths={} active_download_jobs={} active_upload_jobs={}",
        account_prefix,
        cycle_id,
        download_candidates.len(),
        upload_paths.len(),
        desired_delete_remote_paths_count,
        desired_delete_local_paths_count,
        desired_conflict_paths_count,
        active_download_jobs,
        active_upload_jobs,
    );
    let summary = PlannerMaterializationSummary {
        upload_paths,
        delete_remote_paths,
        delete_local_paths,
        conflict_paths,
        desired_download_paths: download_candidates.len(),
        desired_upload_paths: upload_candidates.len(),
        desired_delete_remote_paths: desired_delete_remote_paths_count,
        desired_delete_local_paths: desired_delete_local_paths_count,
        desired_conflict_paths: desired_conflict_paths_count,
        active_download_jobs,
        active_upload_jobs,
    };
    enforce_planner_materialization_invariants(profile_id, &summary)?;
    Ok(summary)
}

fn enforce_planner_materialization_invariants(
    profile_id: &str,
    summary: &PlannerMaterializationSummary,
) -> Result<(), String> {
    if summary.upload_paths.len() != summary.desired_upload_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_upload_paths={} but materialized_upload_paths={}",
            profile_id,
            summary.desired_upload_paths,
            summary.upload_paths.len()
        ));
    }
    if summary.delete_remote_paths.len() != summary.desired_delete_remote_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_delete_remote_paths={} but materialized_delete_remote_paths={}",
            profile_id,
            summary.desired_delete_remote_paths,
            summary.delete_remote_paths.len()
        ));
    }
    if summary.delete_local_paths.len() != summary.desired_delete_local_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_delete_local_paths={} but materialized_delete_local_paths={}",
            profile_id,
            summary.desired_delete_local_paths,
            summary.delete_local_paths.len()
        ));
    }
    if summary.conflict_paths.len() != summary.desired_conflict_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_conflict_paths={} but materialized_conflict_paths={}",
            profile_id,
            summary.desired_conflict_paths,
            summary.conflict_paths.len()
        ));
    }
    if summary.active_download_jobs != summary.desired_download_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_download_paths={} but active_download_jobs={}",
            profile_id,
            summary.desired_download_paths,
            summary.active_download_jobs
        ));
    }
    if summary.active_upload_jobs != summary.desired_upload_paths {
        return Err(format!(
            "Planner materialization invariant failed for profile '{}': desired_upload_paths={} but active_upload_jobs={}",
            profile_id,
            summary.desired_upload_paths,
            summary.active_upload_jobs
        ));
    }
    Ok(())
}

fn materialize_action_jobs(
    profile_id: &str,
    direction: &str,
    desired_item_ids: &std::collections::HashSet<String>,
) -> Result<(), String> {
    for item_id in desired_item_ids {
        upsert_action_job_queued(profile_id, direction, item_id)?;
    }
    prune_unplanned_action_jobs(profile_id, direction, desired_item_ids)?;
    Ok(())
}

fn materialize_planner_download_jobs(profile_id: &str) -> Result<usize, String> {
    let download_candidates = list_sync_file_download_candidates(profile_id)?;
    let mut desired_download_item_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for candidate in &download_candidates {
        desired_download_item_ids.insert(candidate.item_id.clone());
        let _ = upsert_download_job(
            profile_id,
            &candidate.item_id,
            &candidate.path,
            candidate.remote_size,
            candidate.remote_modified_ts,
        )?;
    }

    prune_unplanned_queued_jobs(profile_id, DOWNLOAD_JOB_DIRECTION, &desired_download_item_ids)?;
    Ok(download_candidates.len())
}

fn prune_unplanned_queued_jobs(
    profile_id: &str,
    direction: &str,
    planned_item_ids: &std::collections::HashSet<String>,
) -> Result<usize, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT id, item_id
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2
               AND state IN (?3, ?4)",
        )
        .map_err(|error| format!("Failed preparing unplanned queued jobs query: {error}"))?;
    let rows = statement
        .query_map(
            params![
                profile_id,
                direction,
                DOWNLOAD_JOB_STATE_QUEUED,
                DOWNLOAD_JOB_STATE_RETRY_WAIT
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| format!("Failed querying unplanned queued jobs: {error}"))?;

    let mut stale_job_ids: Vec<i64> = Vec::new();
    for row in rows {
        let (job_id, item_id) =
            row.map_err(|error| format!("Failed reading unplanned queued job row: {error}"))?;
        if !planned_item_ids.contains(&item_id) {
            stale_job_ids.push(job_id);
        }
    }
    drop(statement);

    let mut removed: usize = 0;
    for job_id in stale_job_ids {
        removed += connection
            .execute(
                "DELETE FROM sync_jobs WHERE profile_id = ?1 AND direction = ?2 AND id = ?3",
                params![profile_id, direction, job_id],
            )
            .map_err(|error| format!("Failed deleting stale queued job: {error}"))?;
    }
    Ok(removed)
}

fn prune_unplanned_action_jobs(
    profile_id: &str,
    direction: &str,
    planned_item_ids: &std::collections::HashSet<String>,
) -> Result<usize, String> {
    let connection = open_sync_jobs_connection(profile_id)?;
    let mut statement = connection
        .prepare(
            "SELECT id, item_id
             FROM sync_jobs
             WHERE profile_id = ?1
               AND direction = ?2",
        )
        .map_err(|error| format!("Failed preparing action prune query: {error}"))?;
    let rows = statement
        .query_map(params![profile_id, direction], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("Failed querying action prune rows: {error}"))?;

    let mut stale_job_ids: Vec<i64> = Vec::new();
    for row in rows {
        let (job_id, item_id) = row.map_err(|error| format!("Failed reading action prune row: {error}"))?;
        if !planned_item_ids.contains(&item_id) {
            stale_job_ids.push(job_id);
        }
    }
    drop(statement);

    let mut removed: usize = 0;
    for job_id in stale_job_ids {
        removed += connection
            .execute(
                "DELETE FROM sync_jobs WHERE profile_id = ?1 AND direction = ?2 AND id = ?3",
                params![profile_id, direction, job_id],
            )
            .map_err(|error| format!("Failed deleting stale action job: {error}"))?;
    }
    Ok(removed)
}

#[cfg(test)]
mod job_materializer_tests {
    use super::*;

    fn test_profile_id(label: &str) -> String {
        format!(
            "job-materializer-test-{label}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
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

    fn insert_upload_candidate(profile_id: &str, path: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0,
                    NULL, 0, 1,
                    0, 42, 0, 100,
                    'none', NULL, ?3
                )",
                params![profile_id, path, current_unix_seconds()],
            )
            .expect("insert upload candidate");
    }

    fn insert_download_candidate(profile_id: &str, item_id: &str, path: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0,
                    ?3, 1, 0,
                    99, 0, 200, 0,
                    'none', NULL, ?4
                )",
                params![profile_id, path, item_id, current_unix_seconds()],
            )
            .expect("insert download candidate");
    }

    fn insert_stale_queued_job(profile_id: &str, direction: &str, item_id: &str, path: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        let now = current_unix_seconds();
        connection
            .execute(
                "INSERT INTO sync_jobs (
                    profile_id, direction, item_id, path, remote_size, remote_modified_ts,
                    state, run_state, attempt_count, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 1, 1, 'queued', 'idle', 0, ?5, ?5)",
                params![profile_id, direction, item_id, path, now],
            )
            .expect("insert stale queued job");
    }

    fn insert_delete_remote_candidate(profile_id: &str, path: &str, remote_item_id: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0,
                    ?3, 1, 0,
                    1, 0, 100, 50,
                    'none', NULL, ?4
                )",
                params![profile_id, path, remote_item_id, current_unix_seconds()],
            )
            .expect("insert delete remote candidate");
    }

    fn insert_conflict_candidate(profile_id: &str, path: &str, remote_item_id: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0,
                    ?3, 1, 1,
                    1, 2, 100, 100,
                    'none', NULL, ?4
                )",
                params![profile_id, path, remote_item_id, current_unix_seconds()],
            )
            .expect("insert conflict candidate");
    }

    fn insert_delete_local_candidate(profile_id: &str, path: &str, remote_item_id: &str) {
        let connection = open_sync_jobs_connection(profile_id).expect("open sync jobs db");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, ?2, 0, 0,
                    ?3, 0, 1,
                    0, 1, 0, 100,
                    'none', NULL, ?4
                )",
                params![profile_id, path, remote_item_id, current_unix_seconds()],
            )
            .expect("insert delete local candidate");
    }

    #[test]
    fn materialize_planner_actions_is_idempotent_for_upload_paths() {
        let profile_id = test_profile_id("idempotent");
        clear_profile_rows(&profile_id);
        insert_upload_candidate(&profile_id, "docs/report.txt");

        recompute_sync_file_actions(&profile_id, true).expect("recompute planner actions");
        let first = materialize_planner_actions(&profile_id, "[test]", "cycle-a")
            .expect("materialize first run");
        let second = materialize_planner_actions(&profile_id, "[test]", "cycle-a")
            .expect("materialize second run");

        assert_eq!(first.upload_paths, second.upload_paths);
        assert_eq!(first.delete_remote_paths, second.delete_remote_paths);
        assert_eq!(first.delete_local_paths, second.delete_local_paths);
        assert_eq!(first.conflict_paths, second.conflict_paths);
        assert_eq!(first.active_download_jobs, second.active_download_jobs);
        assert_eq!(first.active_upload_jobs, second.active_upload_jobs);
        assert!(first.upload_paths.contains("docs/report.txt"));
    }

    #[test]
    fn materialize_planner_actions_enqueues_and_prunes_jobs() {
        let profile_id = test_profile_id("enqueue-prune");
        clear_profile_rows(&profile_id);
        insert_upload_candidate(&profile_id, "docs/new-upload.txt");
        insert_download_candidate(&profile_id, "remote-1", "docs/new-download.txt");
        insert_delete_remote_candidate(&profile_id, "docs/remote-delete.txt", "remote-delete-id");
        insert_conflict_candidate(&profile_id, "docs/conflict.txt", "remote-conflict-id");
        insert_stale_queued_job(
            &profile_id,
            DOWNLOAD_JOB_DIRECTION,
            "stale-download-id",
            "docs/stale-download.txt",
        );
        insert_stale_queued_job(
            &profile_id,
            UPLOAD_JOB_DIRECTION,
            "docs/stale-upload.txt",
            "docs/stale-upload.txt",
        );
        insert_stale_queued_job(
            &profile_id,
            DELETE_REMOTE_JOB_DIRECTION,
            "docs/stale-delete-remote.txt",
            "docs/stale-delete-remote.txt",
        );
        insert_stale_queued_job(
            &profile_id,
            CONFLICT_JOB_DIRECTION,
            "docs/stale-conflict.txt",
            "docs/stale-conflict.txt",
        );

        recompute_sync_file_actions(&profile_id, true).expect("recompute planner actions");
        let summary = materialize_planner_actions(&profile_id, "[test]", "cycle-b")
            .expect("materialize planner actions");
        assert_eq!(summary.desired_download_paths, 1);
        assert_eq!(summary.desired_upload_paths, 1);
        assert_eq!(summary.desired_delete_remote_paths, 1);
        assert_eq!(summary.desired_conflict_paths, 1);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let active_download: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = 'download'
                   AND item_id = 'remote-1'
                   AND state IN ('queued', 'in_progress', 'retry_wait')",
                params![&profile_id],
                |row| row.get(0),
            )
            .expect("count active download job");
        let active_upload: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = 'upload'
                   AND item_id = 'docs/new-upload.txt'
                   AND state IN ('queued', 'in_progress', 'retry_wait')",
                params![&profile_id],
                |row| row.get(0),
            )
            .expect("count active upload job");
        let stale_download: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = 'download'
                   AND item_id = 'stale-download-id'",
                params![&profile_id],
                |row| row.get(0),
            )
            .expect("count stale download job");
        let stale_upload: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = 'upload'
                   AND item_id = 'docs/stale-upload.txt'",
                params![&profile_id],
                |row| row.get(0),
            )
            .expect("count stale upload job");
        let active_delete_remote: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = 'docs/remote-delete.txt'
                   AND state IN ('queued', 'in_progress', 'retry_wait')",
                params![&profile_id, DELETE_REMOTE_JOB_DIRECTION],
                |row| row.get(0),
            )
            .expect("count active delete remote action job");
        let active_conflict: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = 'docs/conflict.txt'
                   AND state IN ('queued', 'in_progress', 'retry_wait')",
                params![&profile_id, CONFLICT_JOB_DIRECTION],
                |row| row.get(0),
            )
            .expect("count active conflict action job");
        let stale_delete_remote: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = 'docs/stale-delete-remote.txt'",
                params![&profile_id, DELETE_REMOTE_JOB_DIRECTION],
                |row| row.get(0),
            )
            .expect("count stale delete-remote action job");
        let stale_conflict: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND direction = ?2
                   AND item_id = 'docs/stale-conflict.txt'",
                params![&profile_id, CONFLICT_JOB_DIRECTION],
                |row| row.get(0),
            )
            .expect("count stale conflict action job");

        assert_eq!(active_download, 1);
        assert_eq!(active_upload, 1);
        assert_eq!(active_delete_remote, 1);
        assert_eq!(active_conflict, 1);
        assert_eq!(stale_download, 0);
        assert_eq!(stale_upload, 0);
        assert_eq!(stale_delete_remote, 0);
        assert_eq!(stale_conflict, 0);
    }

    #[test]
    fn materialize_planner_actions_is_idempotent_for_all_action_lanes() {
        let profile_id = test_profile_id("idempotent-all-actions");
        clear_profile_rows(&profile_id);
        insert_upload_candidate(&profile_id, "docs/upload-idempotent.txt");
        insert_download_candidate(&profile_id, "remote-download-idempotent", "docs/download-idempotent.txt");
        insert_delete_remote_candidate(
            &profile_id,
            "docs/delete-remote-idempotent.txt",
            "remote-delete-idempotent",
        );
        insert_delete_local_candidate(
            &profile_id,
            "docs/delete-local-idempotent.txt",
            "remote-delete-local-idempotent",
        );
        insert_conflict_candidate(
            &profile_id,
            "docs/conflict-idempotent.txt",
            "remote-conflict-idempotent",
        );

        recompute_sync_file_actions(&profile_id, true).expect("recompute planner actions");
        let first = materialize_planner_actions(&profile_id, "[test]", "cycle-idempotent-a")
            .expect("first materialize");
        let second = materialize_planner_actions(&profile_id, "[test]", "cycle-idempotent-b")
            .expect("second materialize");

        assert_eq!(first.desired_download_paths, second.desired_download_paths);
        assert_eq!(first.desired_upload_paths, second.desired_upload_paths);
        assert_eq!(first.desired_delete_remote_paths, second.desired_delete_remote_paths);
        assert_eq!(first.desired_delete_local_paths, second.desired_delete_local_paths);
        assert_eq!(first.desired_conflict_paths, second.desired_conflict_paths);

        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(1)
                 FROM sync_jobs
                 WHERE profile_id = ?1
                   AND state IN ('queued', 'in_progress', 'retry_wait')",
                params![&profile_id],
                |row| row.get(0),
            )
            .expect("count active jobs");
        assert_eq!(active_count, 5);
    }

    #[test]
    fn planner_materialization_invariant_rejects_upload_count_mismatch() {
        let summary = PlannerMaterializationSummary {
            upload_paths: std::collections::HashSet::new(),
            delete_remote_paths: std::collections::HashSet::new(),
            delete_local_paths: std::collections::HashSet::new(),
            conflict_paths: Vec::new(),
            desired_download_paths: 0,
            desired_upload_paths: 1,
            desired_delete_remote_paths: 0,
            desired_delete_local_paths: 0,
            desired_conflict_paths: 0,
            active_download_jobs: 0,
            active_upload_jobs: 1,
        };

        let error = enforce_planner_materialization_invariants("profile-test", &summary)
            .expect_err("mismatched upload materialization counts must fail");
        assert!(error.contains("desired_upload_paths=1"));
    }
}
