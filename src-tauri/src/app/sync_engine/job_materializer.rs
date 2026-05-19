struct PlannerMaterializationSummary {
    upload_paths: std::collections::HashSet<String>,
    active_download_jobs: usize,
    active_upload_jobs: usize,
}

fn materialize_planner_actions(
    profile_id: &str,
    account_prefix: &str,
    cycle_id: &str,
) -> Result<PlannerMaterializationSummary, String> {
    let upload_paths: std::collections::HashSet<String> =
        list_sync_file_paths_by_desired_action(profile_id, PLANNER_ACTION_UPLOAD)?
            .into_iter()
            .collect();
    let (active_download_jobs, active_upload_jobs) = read_materialized_job_counts(profile_id)?;
    log::info!(
        "{} [cycle:{}] PLANNER_MATERIALIZATION_SUMMARY upload_paths={} active_download_jobs={} active_upload_jobs={}",
        account_prefix,
        cycle_id,
        upload_paths.len(),
        active_download_jobs,
        active_upload_jobs,
    );
    Ok(PlannerMaterializationSummary {
        upload_paths,
        active_download_jobs,
        active_upload_jobs,
    })
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
        assert_eq!(first.active_download_jobs, second.active_download_jobs);
        assert_eq!(first.active_upload_jobs, second.active_upload_jobs);
        assert!(first.upload_paths.contains("docs/report.txt"));
    }
}
