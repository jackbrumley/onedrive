use crate::app::app_settings;
use crate::app::log_context;
use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::env::consts::{ARCH, OS};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const APP_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
const PROFILE_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024;
const RAW_LOG_MAX_BYTES: u64 = 200 * 1024 * 1024;
const LOG_ROTATION_COUNT: usize = 5;
const RAW_LOG_RETENTION_COUNT: usize = 10;

static APP_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static PROFILE_LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static RAW_LOG_MODE: AtomicBool = AtomicBool::new(false);
static RAW_LOG_SESSION_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

struct SessionLogger;

static LOGGER: SessionLogger = SessionLogger;

impl Log for SessionLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = record.args().to_string();
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let line = format!("[{}] {}: {}", timestamp, record.level(), message);

        match record.level() {
            Level::Error | Level::Warn => eprintln!("{}", line),
            _ => println!("{}", line),
        }

        route_log_line(record.level(), &message, &line);
    }

    fn flush(&self) {}
}

pub fn initialize_session_logging() -> Result<(), String> {
    let debug_dir = get_debug_dir()?;
    let app_log_path = debug_dir.join("app.log");
    let profile_log_dir = debug_dir.join("profiles");
    fs::create_dir_all(&profile_log_dir).map_err(|error| error.to_string())?;

    let _ = APP_LOG_PATH.set(app_log_path);
    let _ = PROFILE_LOG_DIR.set(profile_log_dir);
    let _ = RAW_LOG_SESSION_PATH.set(Mutex::new(None));

    let raw_mode_enabled = app_settings::load_raw_logger_mode().unwrap_or(false);
    RAW_LOG_MODE.store(raw_mode_enabled, Ordering::Relaxed);
    if raw_mode_enabled {
        let _ = ensure_raw_session_path();
    }

    append_app_log_line(&format!(
        "[{}] INFO: SESSION START | app=somedrive version={} platform={}/{}",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        env!("CARGO_PKG_VERSION"),
        OS,
        ARCH
    ));

    Ok(())
}

pub fn initialize_app_logger() -> Result<(), String> {
    set_logger(&LOGGER).map_err(|error| error.to_string())?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}

pub fn resolve_session_log_path() -> Result<PathBuf, String> {
    APP_LOG_PATH
        .get()
        .cloned()
        .or_else(|| get_debug_dir().ok().map(|path| path.join("app.log")))
        .ok_or_else(|| "Session log path unavailable".to_string())
}

#[tauri::command]
pub fn get_raw_logger_mode() -> Result<bool, String> {
    Ok(RAW_LOG_MODE.load(Ordering::Relaxed))
}

#[tauri::command]
pub fn set_raw_logger_mode(enabled: bool) -> Result<(), String> {
    RAW_LOG_MODE.store(enabled, Ordering::Relaxed);
    app_settings::save_raw_logger_mode(enabled)?;

    if enabled {
        let _ = ensure_raw_session_path()?;
    } else if let Some(raw_mutex) = RAW_LOG_SESSION_PATH.get() {
        let mut guard = raw_mutex
            .lock()
            .map_err(|_| "Raw log state lock poisoned".to_string())?;
        *guard = None;
    }

    Ok(())
}

#[tauri::command]
pub fn get_session_log_text() -> Result<String, String> {
    let log_path = resolve_session_log_path()?;
    std::fs::read_to_string(log_path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn copy_session_log_to_clipboard() -> Result<(), String> {
    let logs = get_session_log_text()?;
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(logs).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_session_log() -> Result<(), String> {
    let log_path = resolve_session_log_path()?;
    open_path_with_system(&log_path)
}

#[tauri::command]
pub fn open_profile_log(profile_id: String) -> Result<(), String> {
    let log_path = resolve_profile_log_path(&profile_id)?;
    if !log_path.exists() {
        return Err(format!(
            "Profile log does not exist yet for '{}'. Start a sync and try again.",
            profile_id
        ));
    }
    open_path_with_system(&log_path)
}

fn open_path_with_system(path: &PathBuf) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub fn log_ui_event(message: String) -> Result<(), String> {
    let sanitized = message.trim();
    if sanitized.is_empty() {
        return Ok(());
    }
    log::info!("UI: {}", sanitized);
    Ok(())
}

fn route_log_line(level: Level, message: &str, line: &str) {
    if let Some(account_key) = extract_account_key(message) {
        append_profile_log_line(&account_key, line);
        if matches!(level, Level::Warn | Level::Error)
            || (matches!(level, Level::Info) && is_account_app_activity(message))
        {
            append_app_log_line(line);
        }
    } else {
        append_app_log_line(line);
    }

    if RAW_LOG_MODE.load(Ordering::Relaxed) {
        append_raw_log_line(line);
    }
}

fn is_account_app_activity(message: &str) -> bool {
    const APP_ACTIVITY_MARKERS: [&str; 16] = [
        "SYNC_WORKER_ALREADY_RUNNING",
        "SYNC_WORKER_STARTING",
        "SYNC_WORKER_STOP_SIGNAL",
        "SYNC_WORKER_RESUME_DELAY",
        "SYNC_TICK",
        "SYNC_TICK_SKIPPED",
        "SYNC_HEARTBEAT",
        "SYNC_CYCLE_START",
        "SYNC_CYCLE_COMPLETE",
        "SYNC_CYCLE_FAILED",
        "SYNC_CYCLE_CANCELLED",
        "REMOTE_PIPELINE_START",
        "REMOTE_SCAN_PROGRESS",
        "LOCAL_SCAN_SUMMARY",
        "DOWNLOAD_RETRY_RECOVERED",
        "UPLOAD_RETRY_RECOVERED",
    ];

    APP_ACTIVITY_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

fn append_app_log_line(line: &str) {
    if let Ok(log_path) = resolve_session_log_path() {
        append_line_with_rotation(&log_path, line, APP_LOG_MAX_BYTES, LOG_ROTATION_COUNT);
    }
}

fn append_profile_log_line(account_key: &str, line: &str) {
    if let Ok(path) = resolve_profile_log_path_by_account_key(account_key) {
        append_line_with_rotation(&path, line, PROFILE_LOG_MAX_BYTES, LOG_ROTATION_COUNT);
    }
}

fn append_raw_log_line(line: &str) {
    if let Ok(path) = ensure_raw_session_path() {
        append_line_with_rotation(&path, line, RAW_LOG_MAX_BYTES, 0);
    }
}

fn append_line_with_rotation(path: &PathBuf, line: &str, max_bytes: u64, keep_count: usize) {
    maybe_rotate(path, max_bytes, keep_count);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", line);
    }
}

fn maybe_rotate(path: &PathBuf, max_bytes: u64, keep_count: usize) {
    if keep_count == 0 {
        return;
    }
    let file_size = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if file_size < max_bytes {
        return;
    }

    for index in (1..=keep_count).rev() {
        let from = if index == 1 {
            path.clone()
        } else {
            PathBuf::from(format!("{}.{}", path.display(), index - 1))
        };
        let to = PathBuf::from(format!("{}.{}", path.display(), index));
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
}

fn extract_account_key(message: &str) -> Option<String> {
    let prefix = "[acct:";
    let start = message.find(prefix)?;
    let after_prefix = &message[start + prefix.len()..];
    let end = after_prefix.find(']')?;
    let account_key = after_prefix[..end].trim();
    if account_key.is_empty() {
        return None;
    }
    Some(account_key.to_string())
}

fn sanitize_path_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || character == '.'
        {
            output.push(character);
        } else {
            output.push('_');
        }
    }
    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        "unknown-profile".to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_profile_log_path(profile_id: &str) -> Result<PathBuf, String> {
    let account_key = log_context::account_identity(profile_id);
    resolve_profile_log_path_by_account_key(&account_key)
}

fn resolve_profile_log_path_by_account_key(account_key: &str) -> Result<PathBuf, String> {
    let dir = PROFILE_LOG_DIR
        .get()
        .cloned()
        .or_else(|| get_debug_dir().ok().map(|path| path.join("profiles")))
        .ok_or_else(|| "Profile log directory unavailable".to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let safe_key = sanitize_path_component(account_key);
    Ok(dir.join(format!("{}.log", safe_key)))
}

fn ensure_raw_session_path() -> Result<PathBuf, String> {
    let raw_mutex = RAW_LOG_SESSION_PATH
        .get()
        .ok_or_else(|| "Raw log state unavailable".to_string())?;
    let mut guard = raw_mutex
        .lock()
        .map_err(|_| "Raw log state lock poisoned".to_string())?;

    if let Some(existing) = &*guard {
        return Ok(existing.clone());
    }

    let raw_dir = get_debug_dir()?.join("raw");
    fs::create_dir_all(&raw_dir).map_err(|error| error.to_string())?;
    prune_old_raw_logs(&raw_dir);

    let file_name = format!("raw-{}.log", Local::now().format("%Y-%m-%dT%H-%M-%S"));
    let raw_path = raw_dir.join(file_name);
    *guard = Some(raw_path.clone());
    Ok(raw_path)
}

fn prune_old_raw_logs(raw_dir: &PathBuf) {
    let entries = match fs::read_dir(raw_dir) {
        Ok(value) => value,
        Err(_) => return,
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("log"))
        .collect();
    files.sort();

    if files.len() <= RAW_LOG_RETENTION_COUNT {
        return;
    }

    let remove_count = files.len().saturating_sub(RAW_LOG_RETENTION_COUNT);
    for path in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

fn get_debug_dir() -> Result<PathBuf, String> {
    let debug_dir = dirs::config_dir()
        .ok_or_else(|| "Could not find config directory".to_string())?
        .join("somedrive")
        .join("debug");
    fs::create_dir_all(&debug_dir).map_err(|error| error.to_string())?;
    Ok(debug_dir)
}

fn set_logger(logger: &'static dyn Log) -> Result<(), SetLoggerError> {
    log::set_logger(logger)
}
