// No console window behind the editor on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod commands;
mod config;
mod diff;
mod document;
mod editor_view;
mod fuzzy;
mod fonts;
mod git;
mod highlight;
mod history;
mod icons;
mod index;
mod jobs;
mod keymap;
mod lint;
mod logo;
mod lsp;
mod os;
mod plugins;
mod project;
mod rebase;
mod search;
mod sidebar;
mod split;
mod symbols;
mod terminal;

use eframe::egui;
use std::path::PathBuf;

/// Under WSLg, Wayland + Mesa's software EGL leaves stale (white) areas when the
/// window is resized; XWayland does not. X11 needs libxkbcommon-x11 at runtime,
/// so only switch when it is installed. FLORM_WAYLAND=1 keeps Wayland.
#[cfg(target_os = "linux")]
fn prefer_x11_on_wsl() {
    let wsl = std::fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"));
    if !wsl || std::env::var_os("FLORM_WAYLAND").is_some() || std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let lib = "libxkbcommon-x11.so.0";
    let system = ["/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib"]
        .iter()
        .any(|d| std::path::Path::new(d).join(lib).exists());
    if system {
        std::env::remove_var("WAYLAND_DISPLAY");
        return;
    }
    // Copy installed without sudo (apt-get download + dpkg -x) in ~/.local/share/florm/lib.
    let local = config::home().join(".local/share/florm/lib");
    if local.join(lib).exists() {
        let current = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        if current.split(':').any(|p| std::path::Path::new(p) == local) {
            std::env::remove_var("WAYLAND_DISPLAY");
            return;
        }
        // The dynamic loader reads LD_LIBRARY_PATH only at startup: re-exec ourselves.
        use std::os::unix::process::CommandExt;
        let path = if current.is_empty() { local.display().to_string() } else { format!("{}:{current}", local.display()) };
        if let Ok(exe) = std::env::current_exe() {
            let err = std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env("LD_LIBRARY_PATH", path)
                .env_remove("WAYLAND_DISPLAY")
                .exec();
            eprintln!("florm : relance impossible ({err}), on reste en Wayland");
        }
        return;
    }
    eprintln!("florm : sous WSL, installez libxkbcommon-x11-0 pour éviter les zones blanches au redimensionnement");
}

fn main() -> eframe::Result<()> {
    // Called by `git rebase -i` as sequence editor for an interactive rebase prepared in the UI.
    if std::env::args().nth(1).as_deref() == Some(rebase::PLAN_FLAG) {
        let (plan, todo) = (std::env::args().nth(2), std::env::args().nth(3));
        let ok = matches!((plan, todo), (Some(p), Some(t)) if rebase::write_plan(std::path::Path::new(&p), std::path::Path::new(&t)).is_ok());
        std::process::exit(if ok { 0 } else { 1 });
    }
    // Called by `git rebase -i` as sequence editor when splitting a commit.
    if std::env::args().nth(1).as_deref() == Some(split::TODO_FLAG) {
        let code = match std::env::args().nth(2).map(|f| split::edit_todo(std::path::Path::new(&f))) {
            Some(Ok(())) => 0,
            _ => 1,
        };
        std::process::exit(code);
    }
    #[cfg(target_os = "linux")]
    prefer_x11_on_wsl();
    let arg = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true)
            .with_icon(logo::icon(64))
            .with_title("Florm"),
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "Florm",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, arg)))),
    )
}
