//! Sutty GUI — Native SSH client with a graphical interface.
//! Uses egui/eframe for the window and vt100 for terminal emulation.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icon;
mod ui;

use app::RuttyApp;

fn main() {
    // Log to file so we can debug startup issues even without a console
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("sutty");
    std::fs::create_dir_all(&log_dir).ok();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(
            std::fs::File::create(log_dir.join("sutty-gui.log")).unwrap(),
        )))
        .init();

    log::info!("Starting Sutty GUI");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let icon_data = icon::load_icon(include_bytes!("../icon.ico"));

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([860.0, 520.0])
        .with_title("Sutty — SSH Client")
        .with_resizable(true)
        .with_decorations(true);

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let handle = rt.handle().clone();
    let result = eframe::run_native(
        "Sutty",
        options,
        Box::new(move |_cc| {
            let app = RuttyApp::new(handle.clone());
            Ok(Box::new(app))
        }),
    );

    if let Err(e) = result {
        log::error!("Fatal: {}", e);
        // Show a message box on Windows
        #[cfg(target_os = "windows")]
        {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            let msg: Vec<u16> = OsStr::new(&format!("Sutty failed to start:\n\n{}", e))
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    msg.as_ptr(),
                    windows_sys::w!("Sutty Error"),
                    0x10, // MB_ICONERROR
                );
            }
        }
    }
}
