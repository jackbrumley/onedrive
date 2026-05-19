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
