use crate::app::sync_settings::{load_sync_download_concurrency, save_sync_download_concurrency};

#[tauri::command]
pub fn get_sync_download_concurrency() -> Result<usize, String> {
    load_sync_download_concurrency()
}

#[tauri::command]
pub fn set_sync_download_concurrency(value: usize) -> Result<usize, String> {
    let updated = save_sync_download_concurrency(value)?;
    log::info!("SYNC_SETTING_UPDATED key=download_concurrency value={updated}");
    Ok(updated)
}
