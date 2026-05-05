fn read_linux_distribution_name() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/os-release").ok()?;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

pub fn configure_shell(_app: &mut tauri::App<tauri::Wry>) -> Result<(), Box<dyn std::error::Error>> {
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

    Ok(())
}
