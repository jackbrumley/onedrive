use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 8;
const MIN_DOWNLOAD_CONCURRENCY: usize = 8;
const MAX_DOWNLOAD_CONCURRENCY: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncSettingsFile {
    download_concurrency: usize,
}

impl Default for SyncSettingsFile {
    fn default() -> Self {
        Self {
            download_concurrency: DEFAULT_DOWNLOAD_CONCURRENCY,
        }
    }
}

pub fn load_sync_download_concurrency() -> Result<usize, String> {
    let path = sync_settings_path()?;
    if !path.exists() {
        return Ok(DEFAULT_DOWNLOAD_CONCURRENCY);
    }

    let text = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "Failed reading sync settings '{}': {}",
            path.display(),
            error
        )
    })?;
    let parsed: SyncSettingsFile = serde_json::from_str(&text).map_err(|error| {
        format!(
            "Failed decoding sync settings '{}': {}",
            path.display(),
            error
        )
    })?;
    Ok(clamp_download_concurrency(parsed.download_concurrency))
}

pub fn save_sync_download_concurrency(value: usize) -> Result<usize, String> {
    let normalized = clamp_download_concurrency(value);
    let path = sync_settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed creating sync settings directory '{}': {}",
                parent.display(),
                error
            )
        })?;
    }

    let payload = SyncSettingsFile {
        download_concurrency: normalized,
    };
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("Failed encoding sync settings JSON: {error}"))?;
    std::fs::write(&path, text).map_err(|error| {
        format!(
            "Failed writing sync settings '{}': {}",
            path.display(),
            error
        )
    })?;

    Ok(normalized)
}

pub fn clamp_download_concurrency(value: usize) -> usize {
    value.clamp(MIN_DOWNLOAD_CONCURRENCY, MAX_DOWNLOAD_CONCURRENCY)
}

fn sync_settings_path() -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| "Could not resolve config directory".to_string())?;
    Ok(config_dir.join("somedrive").join("sync_settings.json"))
}
