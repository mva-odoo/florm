// No console window behind the editor on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[macro_use]
mod i18n;

mod app;
mod claude;
mod claude_account;
mod claude_term;
mod commands;
mod config;
mod copilot;
mod conflict;
mod crash;
mod diff;
mod document;
mod editor_view;
mod features;
mod filelist;
mod fuzzy;
mod fonts;
mod git;
mod github;
mod highlight;
mod history;
mod icons;
mod images;
mod index;
mod input;
mod jobs;
mod keymap;
mod lint;
mod local_history;
mod logo;
mod lsp;
mod npm;
mod test_cmd;
mod os;
mod pick;
mod project;
mod rebase;
mod search;
mod shared;
mod skills;
mod sidebar;
mod sound;
mod spell;
mod stat_cache;
mod update;
mod view_bar;
mod symbols;
mod templates;
mod terminal;
mod tools;
mod walk;
mod wrap;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Called by `git rebase -i` as sequence editor for an interactive rebase prepared in the UI.
    if std::env::args().nth(1).as_deref() == Some(rebase::PLAN_FLAG) {
        let (plan, todo) = (std::env::args().nth(2), std::env::args().nth(3));
        let ok = matches!((plan, todo), (Some(p), Some(t)) if rebase::write_plan(std::path::Path::new(&p), std::path::Path::new(&t)).is_ok());
        std::process::exit(if ok { 0 } else { 1 });
    }
    // Called by Claude Code (hooks, status line) for a conversation of the Claude tab in a terminal.
    if claude_term::run_flag(&std::env::args().collect::<Vec<_>>()) {
        std::process::exit(0);
    }
    crash::install();
    #[cfg(target_os = "macos")]
    os::macos_path();
    config::make_private();
    let arg = std::env::args().nth(1).map(PathBuf::from);
    // Florm's own title bar (menus and ─ □ ✕ in the window), see app/title_bar.rs
    let settings = config::load().0;
    let own_title_bar = os::draws_title_bar(settings.show_menu_bar);
    os::TRANSPARENT_WINDOW.store(os::rounded_window(settings.show_menu_bar), std::sync::atomic::Ordering::Relaxed);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true)
            .with_decorations(!own_title_bar)
            // without system decorations, Florm rounds the corners itself (app/title_bar.rs)
            .with_transparent(os::rounded_window(settings.show_menu_bar))
            .with_icon(logo::icon(if cfg!(target_os = "macos") { 256 } else { 64 }, logo::ICON_INSET))
            .with_title("Florm"),
        // Under Wayland, with vsync, eglSwapBuffers waits for a frame callback that a hidden window
        // (other workspace, minimized) never gets: the event loop blocks and the compositor reports
        // "application not responding". Florm only repaints on events, so vsync is not needed.
        vsync: std::env::var_os("WAYLAND_DISPLAY").is_none(),
        // macOS: the menu winit adds quits at Cmd+Q without telling Florm (no question for unsaved
        // files, nor for Claude at work); without it, Cmd+Q is Florm's own Quit, which asks
        #[cfg(target_os = "macos")]
        event_loop_builder: Some(Box::new(|b| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_default_menu(false);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Florm",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, arg)))),
    )
}
