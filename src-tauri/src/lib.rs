mod app;

use app::commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(error) = app::session_log::initialize_session_logging() {
        eprintln!("Failed to initialize session logging: {}", error);
    }
    if let Err(error) = app::session_log::initialize_app_logger() {
        eprintln!("Failed to initialize app logger: {}", error);
    }

    log::info!(
        "Application launch | version={} platform={}/{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app::state::create_app_state())
        .setup(app::bootstrap::configure_shell)
        .invoke_handler(tauri::generate_handler![
            get_status_snapshot,
            check_for_updates,
            list_account_profiles,
            create_account_profile,
            rename_account_profile,
            remove_account_profile,
            set_account_agent_state,
            pause_all_accounts,
            resume_all_accounts,
            set_account_sync_root,
            list_activity_events,
            start_device_auth,
            start_interactive_auth,
            poll_device_auth,
            clear_account_auth,
            app::session_log::log_ui_event,
            app::session_log::get_session_log_text,
            app::session_log::copy_session_log_to_clipboard,
            app::session_log::open_session_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
