#[cfg(test)]
mod local_changes_tests {
    use super::*;

    fn test_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "somedrive-local-changes-test-{label}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("create test temp directory");
        dir
    }

    #[test]
    fn resolve_large_delete_guard_blocks_when_threshold_trips_without_approval() {
        let mut guard_state = LargeDeleteGuardState::default();

        let resolution = resolve_large_delete_guard(
            &mut guard_state,
            vec!["docs/a.txt".to_string(), "docs/b.txt".to_string()],
            vec!["docs/a.txt".to_string(), "docs/b.txt".to_string()],
            2,
        );

        assert_eq!(resolution.blocked_pending_count, Some(2));
        assert!(resolution.triggered_by_threshold);
        assert!(!resolution.clear_issue);
        assert_eq!(guard_state.pending_paths.len(), 2);
        assert!(!guard_state.approved);
    }

    #[test]
    fn resolve_large_delete_guard_consumes_approval_and_clears_pending_paths() {
        let mut guard_state = LargeDeleteGuardState {
            approved: true,
            pending_paths: vec!["docs/a.txt".to_string(), "docs/b.txt".to_string()],
        };

        let resolution = resolve_large_delete_guard(
            &mut guard_state,
            Vec::new(),
            vec!["docs/a.txt".to_string(), "docs/b.txt".to_string()],
            2,
        );

        assert_eq!(resolution.blocked_pending_count, None);
        assert!(resolution.confirmed_by_threshold);
        assert!(resolution.clear_issue);
        assert_eq!(resolution.remote_deleted_count, 2);
        assert!(guard_state.pending_paths.is_empty());
        assert!(!guard_state.approved);
    }

    #[test]
    fn create_safe_backup_creates_incremented_backup_and_preserves_content() {
        let temp_dir = test_temp_dir("safe-backup");
        let source = temp_dir.join("report.txt");
        std::fs::write(&source, b"hello world").expect("write source file");

        let first_backup = create_safe_backup(&source)
            .expect("create first backup")
            .expect("first backup path exists");
        let second_backup = create_safe_backup(&source)
            .expect("create second backup")
            .expect("second backup path exists");

        assert!(first_backup.exists());
        assert!(second_backup.exists());
        assert_ne!(first_backup, second_backup);

        let original_content = std::fs::read(&source).expect("read source");
        let first_content = std::fs::read(&first_backup).expect("read first backup");
        let second_content = std::fs::read(&second_backup).expect("read second backup");
        assert_eq!(original_content, first_content);
        assert_eq!(original_content, second_content);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn large_delete_guard_issue_message_formats_count() {
        let message = large_delete_guard_issue_message(3);
        assert!(message.contains("3 items"));
    }

    #[test]
    fn is_safe_backup_artifact_recognizes_safe_backup_suffix() {
        assert!(is_safe_backup_artifact("docs/report-safeBackup-20260101-120000-0001"));
        assert!(!is_safe_backup_artifact("docs/report.txt"));
    }

    #[test]
    fn collect_remote_delete_candidate_paths_reads_from_sync_files_authority() {
        let profile_id = format!(
            "local-changes-remote-delete-candidates-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let connection = open_sync_jobs_connection(&profile_id).expect("open sync jobs db");
        connection
            .execute("DELETE FROM sync_files WHERE profile_id = ?1", params![&profile_id])
            .expect("clear sync_files rows");

        let now = current_unix_seconds();
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, 'docs/remote-present.txt', 0, 0,
                    'remote-id', 1, 0,
                    1, 0, 100, 0,
                    'none', NULL, ?2
                )",
                params![&profile_id, now],
            )
            .expect("insert remote-present sync_files row");
        connection
            .execute(
                "INSERT INTO sync_files (
                    profile_id, path, is_dir, is_shared_reference,
                    remote_item_id, remote_present, local_present,
                    remote_size, local_size, remote_modified_ts, local_modified_ts,
                    desired_action, conflict_state, updated_at
                ) VALUES (
                    ?1, 'docs/local-only.txt', 0, 0,
                    NULL, 0, 1,
                    0, 1, 0, 100,
                    'none', NULL, ?2
                )",
                params![&profile_id, now],
            )
            .expect("insert local-only sync_files row");

        let candidates = collect_remote_delete_candidate_paths(
            &profile_id,
            &[
                "docs/remote-present.txt".to_string(),
                "docs/local-only.txt".to_string(),
            ],
        )
        .expect("collect remote delete candidate paths");

        assert_eq!(candidates, vec!["docs/remote-present.txt".to_string()]);
    }
}
