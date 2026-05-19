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
