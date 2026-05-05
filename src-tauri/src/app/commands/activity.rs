use crate::app::activity_log::{list_events, ActivityEvent};

#[tauri::command]
pub fn list_activity_events(limit: Option<usize>) -> Result<Vec<ActivityEvent>, String> {
    let max = limit.unwrap_or(120).clamp(1, 500);
    list_events(max)
}
