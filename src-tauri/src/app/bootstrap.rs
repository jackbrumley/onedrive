use crate::app::account_profiles::load_profiles;
use crate::app::log_context;
use crate::app::state::AppState;
use crate::app::sync_engine;
use crate::app::sync_runtime;
use tauri::Manager;

fn read_linux_distribution_name() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/os-release").ok()?;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

pub fn configure_shell(app: &mut tauri::App<tauri::Wry>) -> Result<(), Box<dyn std::error::Error>> {
    sync_runtime::initialize_status_event_stream(app.handle().clone());

    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let x11_display = std::env::var("DISPLAY").ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();

    log::info!(
        "Startup context | version={} platform={}/{} session_type={:?} wayland={:?} x11={:?} desktop={:?}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        session_type,
        wayland_display,
        x11_display,
        desktop
    );

    if let Some(distro_name) = read_linux_distribution_name() {
        log::info!("Linux distribution | {}", distro_name);
    }

    let app_state = app.state::<AppState>();

    match load_profiles() {
        Ok(accounts) => {
            log::info!("Configured accounts at startup | count={}", accounts.len());
            let mut resumed_count = 0_u32;
            let mut paused_count = 0_u32;
            let mut skipped_count = 0_u32;
            let mut drained_jobs_total = 0_usize;

            for account in accounts {
                let prefix = log_context::account_prefix_from_parts(&account.id, &account.email);
                log::info!(
                    "{} Startup account | id={} name={} kind={} auth_configured={} agent_state={} sync_root={}",
                    prefix,
                    account.id,
                    account.display_name,
                    account.kind,
                    account.auth_configured,
                    account.agent_state,
                    account.sync_root,
                );

                sync_engine::runtime_set_profile_auth_ready(
                    &app_state.sync_runtime,
                    &account.id,
                    account.auth_configured,
                );
                let engine_state = if account.agent_state == "syncing" {
                    "running"
                } else {
                    "paused"
                };
                sync_engine::runtime_set_engine_state(
                    &app_state.sync_runtime,
                    &account.id,
                    engine_state,
                );

                if account.agent_state == "syncing" {
                    if account.auth_configured {
                        match sync_engine::prepare_startup_sync_resume(&account.id) {
                            Ok(cleared_jobs) => {
                                drained_jobs_total =
                                    drained_jobs_total.saturating_add(cleared_jobs);
                            }
                            Err(error) => {
                                log::warn!("{} STARTUP_SYNC_PREP_FAILED {}", prefix, error);
                            }
                        }
                        match sync_engine::on_agent_state_changed(
                            &app_state,
                            &account.id,
                            "syncing",
                        ) {
                            Ok(()) => {
                                resumed_count += 1;
                                log::info!("{} STARTUP_SYNC_RESTORED", prefix);
                            }
                            Err(error) => {
                                skipped_count += 1;
                                log::error!("{} STARTUP_SYNC_RESTORE_FAILED {}", prefix, error);
                            }
                        }
                    } else {
                        skipped_count += 1;
                        log::warn!("{} STARTUP_SYNC_SKIPPED reason=auth_not_configured", prefix);
                    }
                } else if account.agent_state == "paused" {
                    paused_count += 1;
                }
            }

            log::info!(
                "Startup sync restore summary | resumed={} paused={} skipped={} startup_drained_jobs={}",
                resumed_count,
                paused_count,
                skipped_count,
                drained_jobs_total
            );
        }
        Err(error) => {
            log::error!("Failed to load configured accounts at startup: {}", error);
        }
    }

    Ok(())
}
