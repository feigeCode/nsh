//! Runs the panel example: opens a panel window, then the app's main
//! window.
//!
//! `Panel::open` runs on the main thread, as `run_app` does, so the runner
//! opens the panel before it starts the app; components run on the engine
//! thread and cannot open windows.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    // nsh.json's id: the window's and where the app keeps its data.
    let app_id = "com.longbridge.nsh.panel";
    let panel = match platform::Panel::open(
        platform::PanelOptions {
            title: "Panel".to_owned(),
            size: Some((420, 300)),
            // A title bar to close the example with; the default is none.
            borderless: false,
            movable_by_background: true,
            ..Default::default()
        },
        platform::AppSource::Rust(platform::Root::of::<panel_native::PanelContent>()),
    ) {
        Ok(panel) => panel,
        Err(error) => {
            eprintln!("can't open the panel window: {error}");
            std::process::exit(1);
        }
    };
    panel.show();
    let code = platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<panel_native::PanelApp>()),
        platform::AppOptions {
            title: "Panel Example".to_owned(),
            app_id: app_id.to_owned(),
            size: (720, 480),
            ..Default::default()
        },
    );
    drop(panel);
    code
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!(
    "Run this binary on macOS, Windows or Linux; use nsh run for iOS, Android and the web."
);
