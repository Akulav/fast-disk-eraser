#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Fast Disk Eraser — securely erase files, folders and free disk space. Made by Akulav.

mod app;
mod cli;
mod core;
mod ui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--cli") {
        attach_parent_console();
        std::process::exit(cli::run(&args[1..]));
    }
    if args.iter().any(|a| a == "--version") {
        attach_parent_console();
        println!("{} {} - made by {}", app::update::APP_NAME, app::update::CURRENT_VERSION, app::update::AUTHOR);
        return Ok(());
    }
    let startup = match app::install::on_startup(&args) {
        app::install::Startup::Exit => return Ok(()),
        app::install::Startup::Run(info) => info,
    };

    let icon = image::load_from_memory(include_bytes!("../assets/icon.png")).ok().map(|img| {
        let img = img.to_rgba8();
        egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
    });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Fast Disk Eraser")
        .with_app_id("fast-disk-eraser")
        .with_inner_size([1120.0, 720.0])
        .with_min_inner_size([900.0, 600.0])
        .with_decorations(false)
        .with_transparent(false)
        .with_drag_and_drop(true)
        .with_resizable(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    // Window size/position live in the app's own folder, not in %APPDATA% / ~/.local/share.
    let persistence_path = app::paths::Paths::detect().ok().map(|p| p.window_file());
    let options = eframe::NativeOptions { viewport, centered: true, persistence_path, ..Default::default() };
    eframe::run_native("Fast Disk Eraser", options, Box::new(move |cc| Ok(Box::new(ui::app::EraserApp::new(cc, startup)))))
}

/// Release builds use the GUI subsystem; reattach to the terminal so `--cli` output is visible.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 call with a constant argument.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
