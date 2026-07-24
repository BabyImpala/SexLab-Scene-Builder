#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod cli;
mod furniture;
mod project;
mod racekeys;
mod window_geometry;

use log::{error, info};
use once_cell::sync::Lazy;
use project::{package::{ExportKind, Package}, position::Position, scene::Scene, stage::Stage, NanoID};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use tauri::{
    menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, SubmenuBuilder},
    AppHandle, Emitter, Listener, Manager, Runtime, Theme, WebviewWindowBuilder, Wry,
};
use tauri_plugin_cli::CliExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use crate::project::position_info::PositionInfo;

const DEFAULT_MAINWINDOW_TITLE: &str = "SexLab Scene Builder";

#[derive(Debug, Serialize, Clone)]
struct ProjectUpdatePayload<'a> {
    scenes: &'a indexmap::IndexMap<NanoID, Scene>,
    pack_name: &'a str,
    pack_author: &'a str,
}

fn emit_project_update<R: Runtime>(emitter: &impl Emitter<R>, prjct: &Package) {
    let payload = ProjectUpdatePayload {
        scenes: &prjct.scenes,
        pack_name: &prjct.pack_name,
        pack_author: &prjct.pack_author,
    };
    if let Err(e) = emitter.emit("on_project_update", &payload) {
        error!("Failed to emit on_project_update: {}", e);
    }
}

fn export_tip_pref_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("SexLabSceneBuilder").join("hide_export_clip_tip"))
}

fn is_export_clip_tip_hidden() -> bool {
    export_tip_pref_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn set_export_clip_tip_hidden(hidden: bool) {
    let Some(path) = export_tip_pref_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, if hidden { "1" } else { "0" });
}

fn run_export(app: AppHandle, kind: ExportKind) {
    tauri::async_runtime::spawn(async move {
        let prjct = PROJECT.lock().unwrap();
        if let Err(err) = prjct.export_as(&app, kind) {
            if err == "Export cancelled" {
                return;
            }
            error!("Failed to export project: {}", err);
            app.dialog()
                .message(&err)
                .title("Export failed")
                .kind(MessageDialogKind::Error)
                .buttons(MessageDialogButtons::Ok)
                .show(|_| {});
        }
    });
}

/// Show the Pandora clip-folder tip (unless dismissed), then open the export picker.
fn start_export_with_tip(app: &AppHandle, kind: ExportKind) {
    if is_export_clip_tip_hidden() {
        run_export(app.clone(), kind);
        return;
    }

    let fnis_mod = PROJECT.lock().unwrap().fnis_mod_name();
    let message = format!(
        "Export writes AnimLists, Behavior files, and registry data — not your .hkx animation clips.\n\n\
         Copy your animation HKX files into:\n\
         meshes/actors/<race>/animations/{fnis_mod}/\n\n\
         For humans that is usually:\n\
         meshes/actors/character/animations/{fnis_mod}/\n\n\
         Pandora only plays clips that live in the folder the Behavior references."
    );

    let app_continue = app.clone();
    app.dialog()
        .message(message)
        .title("Animation clips for Pandora")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Continue".into(),
            "Cancel".into(),
        ))
        .show(move |proceed| {
            if !proceed {
                return;
            }
            let app_hide = app_continue.clone();
            app_continue
                .dialog()
                .message("Don't show this tip again on export?")
                .title("Export tip")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::YesNo)
                .show(move |hide| {
                    if hide {
                        set_export_clip_tip_hidden(true);
                    }
                    run_export(app_hide, kind);
                });
        });
}

pub static PROJECT: Lazy<Mutex<Package>> = Lazy::new(|| {
    let prjct = Package::new();
    Mutex::new(prjct)
});

static EDITED: AtomicBool = AtomicBool::new(false);
#[inline]
fn set_edited(val: bool) -> () {
    EDITED.store(val, Ordering::Relaxed)
}
#[inline]
fn get_edited() -> bool {
    EDITED.load(Ordering::Relaxed)
}

static IS_DARKMODE: AtomicBool = AtomicBool::new(false);
#[inline]
fn set_darkmode(val: bool) -> () {
    IS_DARKMODE.store(val, Ordering::Relaxed)
}
#[inline]
fn get_darkmode() -> bool {
    IS_DARKMODE.load(Ordering::Relaxed)
}

static FOLLOW_OS_THEME: AtomicBool = AtomicBool::new(true);
#[inline]
fn set_follow_os_theme(val: bool) {
    FOLLOW_OS_THEME.store(val, Ordering::Relaxed)
}
#[inline]
fn get_follow_os_theme() -> bool {
    FOLLOW_OS_THEME.load(Ordering::Relaxed)
}

/// Cached OS dark/light. After forced Light, Tao's theme() can still report Light
/// until the stripped GTK name is restored — needed when returning to System.
static LAST_OS_DARK: AtomicBool = AtomicBool::new(false);
#[inline]
fn set_last_os_dark(val: bool) {
    LAST_OS_DARK.store(val, Ordering::Relaxed)
}
#[inline]
fn get_last_os_dark() -> bool {
    LAST_OS_DARK.load(Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
static SAVED_GTK_THEME: Mutex<Option<String>> = Mutex::new(None);

#[cfg(target_os = "linux")]
const GTK_DARK_SUFFIXES: &[&str] = &["-dark", "-Dark", ":dark", "-darker", "-Darker"];

fn theme_for(is_dark: bool) -> Theme {
    if is_dark {
        Theme::Dark
    } else {
        Theme::Light
    }
}

fn set_menu_checked<R: Runtime>(submenu: &tauri::menu::Submenu<R>, id: &str, checked: bool) {
    if let Some(item) = submenu.get(id) {
        if let Some(check) = item.as_check_menuitem() {
            let _ = check.set_checked(checked);
        }
    }
}

fn theme_submenu_from_menu<R: Runtime>(menu: &Menu<R>) -> Option<tauri::menu::Submenu<R>> {
    let items = menu.items().ok()?;
    for item in items {
        let Some(view) = item.as_submenu() else {
            continue;
        };
        if view.get(THEME_SYSTEM).is_some() {
            return Some(view.clone());
        }
        if let Ok(subs) = view.items() {
            for sub in subs {
                if let Some(theme_menu) = sub.as_submenu() {
                    if theme_menu.get(THEME_SYSTEM).is_some() {
                        return Some(theme_menu.clone());
                    }
                }
            }
        }
    }
    None
}

fn update_theme_menu<R: Runtime>(app: &AppHandle<R>) {
    // Menu is attached to the main window, not the app handle.
    let menu = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|w| w.menu())
        .or_else(|| app.menu());
    let Some(menu) = menu else {
        return;
    };
    let Some(theme_menu) = theme_submenu_from_menu(&menu) else {
        return;
    };

    let follow = get_follow_os_theme();
    let is_dark = get_darkmode();
    set_menu_checked(&theme_menu, THEME_SYSTEM, follow);
    set_menu_checked(&theme_menu, THEME_LIGHT, !follow && !is_dark);
    set_menu_checked(&theme_menu, THEME_DARK, !follow && is_dark);
}

#[cfg(target_os = "linux")]
fn restore_saved_gtk_theme(settings: &gtk::Settings) {
    use gtk::prelude::GtkSettingsExt;
    if let Ok(mut saved) = SAVED_GTK_THEME.lock() {
        if let Some(name) = saved.take() {
            settings.set_gtk_theme_name(Some(name.as_str()));
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_linux_gtk_theme(follow_os: bool, is_dark: bool) {
    use gtk::prelude::GtkSettingsExt;

    let Some(settings) = gtk::Settings::default() else {
        return;
    };

    if follow_os || is_dark {
        settings.set_gtk_application_prefer_dark_theme(is_dark);
        restore_saved_gtk_theme(&settings);
        return;
    }

    // Tao's set_theme(Light) only clears prefer-dark; strip *-dark for light CSD.
    settings.set_gtk_application_prefer_dark_theme(false);
    if let Some(theme) = settings.gtk_theme_name() {
        let name = theme.as_str();
        if let Some(base) = GTK_DARK_SUFFIXES
            .iter()
            .find_map(|suffix| name.strip_suffix(suffix))
        {
            if let Ok(mut saved) = SAVED_GTK_THEME.lock() {
                if saved.is_none() {
                    *saved = Some(name.to_string());
                }
            }
            settings.set_gtk_theme_name(Some(base));
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_gtk_theme(_follow_os: bool, _is_dark: bool) {}

fn apply_window_chrome_theme<R: Runtime>(app: &AppHandle<R>, follow_os: bool, is_dark: bool) {
    let theme = if follow_os {
        None
    } else {
        Some(theme_for(is_dark))
    };
    app.set_theme(theme);
    for window in app.webview_windows().values() {
        let _ = window.set_theme(theme);
    }
    apply_linux_gtk_theme(follow_os, is_dark);
}

fn apply_color_theme<R: Runtime>(app: &AppHandle<R>, is_dark: bool, follow_os: bool) {
    set_follow_os_theme(follow_os);
    set_darkmode(is_dark);
    apply_window_chrome_theme(app, follow_os, is_dark);
    update_theme_menu(app);
    if let Err(err) = app.emit("toggle_darkmode", is_dark) {
        error!("Unable to emit theme change: {}", err);
    }
}

/// Restore GTK before clearing preferred_theme so theme() can see *-dark again.
fn apply_system_theme<R: Runtime>(app: &AppHandle<R>) {
    let cached_os_dark = get_last_os_dark();
    apply_linux_gtk_theme(true, cached_os_dark);
    app.set_theme(None);
    for window in app.webview_windows().values() {
        let _ = window.set_theme(None);
    }
    let os_dark = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|w| w.theme().ok())
        .map(|t| matches!(t, Theme::Dark))
        .unwrap_or(cached_os_dark);
    set_last_os_dark(os_dark);
    apply_color_theme(app, os_dark, true);
}

fn apply_os_theme_event<R: Runtime>(app: &AppHandle<R>, theme: Theme) {
    let is_dark = matches!(theme, Theme::Dark);
    if !get_follow_os_theme() {
        // Re-assert forced chrome; do not treat this as an OS theme sample.
        apply_window_chrome_theme(app, false, get_darkmode());
        return;
    }
    set_last_os_dark(is_dark);
    if get_darkmode() == is_dark {
        update_theme_menu(app);
        return;
    }
    set_darkmode(is_dark);
    update_theme_menu(app);
    if let Err(err) = app.emit("toggle_darkmode", is_dark) {
        error!("Unable to emit OS theme change: {}", err);
    }
}

fn sync_theme_from_window<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    match window.theme() {
        Ok(theme) => {
            if get_follow_os_theme() {
                apply_os_theme_event(window.app_handle(), theme);
            } else {
                apply_window_chrome_theme(window.app_handle(), false, get_darkmode());
            }
        }
        Err(err) => error!("Unable to read window theme: {}", err),
    }
}

fn setup_logger() -> Result<(), fern::InitError> {
    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| out.finish(format_args!("[{}] {}", record.level(), message)))
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout());

    // Try to create log file in user's data directory, fall back to stdout-only if not possible
    if let Some(data_dir) = dirs::data_local_dir() {
        let log_dir = data_dir.join("SexLabSceneBuilder");
        if std::fs::create_dir_all(&log_dir).is_ok() {
            let log_path = log_dir.join("SceneBuilder.log");
            if let Ok(log_file) = fern::log_file(&log_path) {
                dispatch = dispatch.chain(log_file);
            }
        }
    }

    dispatch.apply()?;
    Ok(())
}

/// MAIN

const MAIN_WINDOW: &str = "main_window";

const NEW_PROJECT: &str = "new_prjct";
const OPEN_PROJECT: &str = "open_prjct";
const IMPORT_SLAL: &str = "import_slal";
const ENRICH_SLANIM: &str = "enrich_slanim";
const ENRICH_FNIS: &str = "enrich_fnis";
const THEME_SYSTEM: &str = "theme_system";
const THEME_LIGHT: &str = "theme_light";
const THEME_DARK: &str = "theme_dark";

fn save_and_exit<R: Runtime>(app: &AppHandle<R>) {
    window_geometry::save_all_window_geometry(app);
    app.exit(0);
}

fn main() {
    setup_logger().expect("Unable to initialize logger");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_cli::init())
        .invoke_handler(tauri::generate_handler![
            request_project_update,
            set_pack_name,
            set_pack_author,
            get_race_keys,
            create_blank_scene,
            save_scene,
            delete_scene,
            open_stage_editor,
            open_stage_editor_from,
            stage_save_and_close,
            make_position,
            mark_as_edited,
            get_in_darkmode
        ])
        .setup(|app| {
            let matches = app.cli().matches()?;
            if let Some(command) = matches.subcommand {
                let res = match command.name.as_str() {
                    "convert" => cli::convert(command.matches.args),
                    "build" => cli::build(command.matches.args),
                    "export-slal" => cli::export_slal(command.matches.args),
                    "generate-behaviors" => cli::generate_behaviors(command.matches.args),
                    _ => Err(format!("Unrecognized subcommand: {}", command.name)),
                };
                if let Err(e) = &res {
                    error!("Error while processing CLI command: {}", e);
                }
                // Exit here so CLI never falls through into the GTK event loop
                // (needed for headless generate-behaviors / CI smoke tests).
                std::process::exit(res.is_err() as i32);
            }
            let main_window = WebviewWindowBuilder::new(
                app.app_handle(),
                MAIN_WINDOW.to_string(),
                tauri::WebviewUrl::App("./index.html".into()),
            )
            .title(DEFAULT_MAINWINDOW_TITLE)
            .menu(get_menu(&app.app_handle()).expect("Failed to create menu"))
            .min_inner_size(800.0, 500.0)
            .inner_size(1280.0, 720.0)
            .build()
            .expect("Failed to create main window");
            window_geometry::restore_window_geometry(&main_window);
            set_follow_os_theme(true);
            app.app_handle().set_theme(None);
            sync_theme_from_window(&main_window);
            app.on_menu_event(menu_event_listener);
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::ThemeChanged(theme) => {
                    apply_os_theme_event(window.app_handle(), *theme);
                }
                tauri::WindowEvent::CloseRequested { api, .. }
                    if window.label() == MAIN_WINDOW =>
                {
                    // Always prevent first — blocking dialogs on the GTK main thread
                    // freeze the app on Linux (especially after a second webview existed).
                    api.prevent_close();
                    let app = window.app_handle().clone();
                    if get_edited() {
                        app.dialog()
                            .message(
                                "There are unsaved changes. Are you sure you want to close?",
                            )
                            .title("Close")
                            .buttons(MessageDialogButtons::YesNo)
                            .kind(MessageDialogKind::Warning)
                            .show(move |should_close| {
                                if should_close {
                                    save_and_exit(&app);
                                }
                            });
                    } else {
                        save_and_exit(&app);
                    }
                }
                tauri::WindowEvent::CloseRequested { .. }
                    if window.label().starts_with("stage_editor_") =>
                {
                    window_geometry::save_window_geometry_by_label(
                        window.app_handle(),
                        window.label(),
                    );
                }
                tauri::WindowEvent::Destroyed
                    if window.label().starts_with("stage_editor_") =>
                {
                    unblock_main_if_no_stage_editors(window.app_handle());
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn reload_project(reload_type: &str, window: &tauri::WebviewWindow) {
    let mut prjct = PROJECT.lock().unwrap();
    let result = match reload_type {
        NEW_PROJECT => {
            prjct.reset();
            Ok(())
        }
        OPEN_PROJECT => prjct.load_project(window.app_handle()),
        IMPORT_SLAL => prjct.load_slal(window.app_handle()),
        _ => Err(format!("Invalid reload type: {}", reload_type)),
    };

    if let Err(e) = result {
        error!("{}", e);
        window
            .app_handle()
            .dialog()
            .message(&e)
            .title("Load failed")
            .kind(MessageDialogKind::Error)
            .buttons(MessageDialogButtons::Ok)
            .show(|_| {});
        return;
    }
    if prjct.pack_name == String::default() {
        let _ = window.set_title(DEFAULT_MAINWINDOW_TITLE);
    } else {
        let _ = window
            .set_title(format!("{} - {}", DEFAULT_MAINWINDOW_TITLE, prjct.pack_name).as_str());
    }
    // Import leaves an unsaved in-memory project until Save As
    set_edited(reload_type == IMPORT_SLAL);
    emit_project_update(window, &prjct);
}

fn get_menu(app: &AppHandle) -> Result<Menu<Wry>, Box<dyn std::error::Error>> {
    let file_menu = SubmenuBuilder::new(app, "File")
        .items(&[
            &MenuItem::with_id(
                app,
                NEW_PROJECT,
                "New Project",
                true,
                "cmdOrControl+N".into(),
            )?,
            &MenuItem::with_id(
                app,
                OPEN_PROJECT,
                "Open Project",
                true,
                "cmdOrControl+O".into(),
            )?,
            &MenuItem::with_id(
                app,
                IMPORT_SLAL,
                "Import SLAL...",
                true,
                Option::<&str>::None,
            )?,
            &MenuItem::with_id(
                app,
                ENRICH_SLANIM,
                "Enrich from SLAnim source...",
                true,
                Option::<&str>::None,
            )?,
            &MenuItem::with_id(
                app,
                ENRICH_FNIS,
                "Enrich from FNIS AnimList...",
                true,
                Option::<&str>::None,
            )?,
        ])
        .separator()
        .items(&[
            &MenuItem::with_id(
                app,
                "import_offset",
                "Import Offset.yaml",
                true,
                Option::<&str>::None,
            )?,
            &MenuItem::with_id(app, "save", "Save", true, "cmdOrControl+S".into())?,
            &MenuItem::with_id(
                app,
                "save_as",
                "Save As...",
                true,
                "cmdOrControl+Shift+S".into(),
            )?,
        ])
        .separator()
        .item(
            &SubmenuBuilder::new(app, "Export")
                .items(&[
                    &MenuItem::with_id(
                        app,
                        "export_both",
                        "SLSB + SLAL...",
                        true,
                        "cmdOrControl+B".into(),
                    )?,
                    &MenuItem::with_id(
                        app,
                        "export_slsb",
                        "SLSB only...",
                        true,
                        Option::<&str>::None,
                    )?,
                    &MenuItem::with_id(
                        app,
                        "export_slal",
                        "SLAL only...",
                        true,
                        Option::<&str>::None,
                    )?,
                ])
                .build()?,
        )
        .separator()
        .quit()
        .build()?;
    let theme_menu = SubmenuBuilder::new(app, "Theme")
        .item(&CheckMenuItem::with_id(
            app,
            THEME_SYSTEM,
            "System",
            true,
            get_follow_os_theme(),
            Option::<&str>::None,
        )?)
        .item(&CheckMenuItem::with_id(
            app,
            THEME_LIGHT,
            "Light",
            true,
            !get_follow_os_theme() && !get_darkmode(),
            Option::<&str>::None,
        )?)
        .item(&CheckMenuItem::with_id(
            app,
            THEME_DARK,
            "Dark",
            true,
            !get_follow_os_theme() && get_darkmode(),
            Option::<&str>::None,
        )?)
        .build()?;
    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&theme_menu)
        .build()?;
    let help_menu = SubmenuBuilder::new(app, "Help")
        .text("open_docs", "Open Wiki")
        .separator()
        .text("about", "About")
        .separator()
        .text("discord", "Discord")
        .text("patreon", "Patreon")
        .text("kofi", "Ko-Fi")
        .build()?;
    let top_menu = MenuBuilder::new(app)
        .items(&[&file_menu, &view_menu, &help_menu])
        .build()?;
    Ok(top_menu)
}

fn menu_event_listener(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().0.as_str() {
        NEW_PROJECT | OPEN_PROJECT | IMPORT_SLAL => {
            let event_id = event.id().0.clone();
            let window = app.get_webview_window(MAIN_WINDOW).unwrap();
            let title = match event_id.as_str() {
                NEW_PROJECT => "New Project",
                OPEN_PROJECT => "Open Project",
                _ => "Import SLAL",
            };
            // blocking_* dialogs must not run on the GTK menu/main thread (Linux freeze).
            let start_reload = move || {
                let event_id = event_id.clone();
                let window = window.clone();
                tauri::async_runtime::spawn(async move {
                    reload_project(&event_id, &window);
                });
            };
            if get_edited() {
                app.dialog()
                    .message("There are unsaved changes. Loading a new project will cause these changes to be lost.\nContinue?")
                    .title(title)
                    .buttons(MessageDialogButtons::YesNo)
                    .kind(MessageDialogKind::Warning)
                    .show(move |result| match result {
                        true => start_reload(),
                        false => info!("User cancelled the project reload.")
                    });
                return;
            }
            start_reload();
        }
        "save" | "save_as" => {
            let save_as = event.id().0 == "save_as";
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut prjct = PROJECT.lock().unwrap();
                if let Err(err) = prjct.save_project(save_as, &app) {
                    error!("Failed to save project: {}", err);
                    return;
                }
                set_edited(false);
                let window = app.get_webview_window(MAIN_WINDOW).unwrap();
                if prjct.pack_name.is_empty() {
                    let _ = window.set_title(DEFAULT_MAINWINDOW_TITLE);
                } else {
                    let _ = window.set_title(
                        format!("{} - {}", DEFAULT_MAINWINDOW_TITLE, prjct.pack_name).as_str(),
                    );
                }
            });
        }
        "export_both" | "export_slsb" | "export_slal" => {
            let kind = match event.id().0.as_str() {
                "export_slsb" => ExportKind::Slsb,
                "export_slal" => ExportKind::Slal,
                _ => ExportKind::Both,
            };
            start_export_with_tip(app, kind);
        }
        THEME_SYSTEM => {
            apply_system_theme(app);
        }
        THEME_LIGHT => {
            apply_color_theme(app, false, false);
        }
        THEME_DARK => {
            apply_color_theme(app, true, false);
        }
        "open_docs" => {
            let _ = app.opener().open_url(
                "https://slp-community.github.io/SexLab-Wiki/slsb/creating-packs-using-slsb/",
                Option::<String>::None,
            );
        }
        "about" => {
            let msg = format!(
                "SexLab Scene Builder {}\n\
                 Apache-2.0 — Scrab and contributors\n\
                 https://github.com/SLP-Community/SexLab-Scene-Builder\n\n\
                 Third-party:\n\
                 • serde-hkx (MIT OR Apache-2.0) — Behavior.hkx packing\n\
                   https://github.com/SARDONYX-sard/serde-hkx\n\
                   Copyright SARDONYX and contributors",
                env!("CARGO_PKG_VERSION")
            );
            app.dialog()
                .message(msg)
                .title("About SexLab Scene Builder")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::Ok)
                .show(|_| {});
        }
        "discord" => {
            let _ = app
                .opener()
                .open_url("https://discord.gg/JPSHb4ebqj", Option::<String>::None);
        }
        "patreon" => {
            let _ = app.opener().open_url(
                "https://www.patreon.com/ScrabJoseline",
                Option::<String>::None,
            );
        }
        "kofi" => {
            let _ = app
                .opener()
                .open_url("https://ko-fi.com/scrab", Option::<String>::None);
        }
        "import_offset" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut prjct = PROJECT.lock().unwrap();
                if let Err(err) = prjct.import_offset(&app) {
                    error!("{}", err);
                }
            });
        }
        ENRICH_SLANIM => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut prjct = PROJECT.lock().unwrap();
                match prjct.enrich_from_slanim_source(&app) {
                    Ok(summary) => {
                        set_edited(true);
                        let window = app.get_webview_window(MAIN_WINDOW).unwrap();
                        emit_project_update(&window, &prjct);
                        let kind = if summary.positions_updated > 0 {
                            MessageDialogKind::Info
                        } else {
                            MessageDialogKind::Warning
                        };
                        app.dialog()
                            .message(summary.message())
                            .title("Enrich from SLAnim source")
                            .kind(kind)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    Err(err) => {
                        error!("{}", err);
                        app.dialog()
                            .message(&err)
                            .title("Enrich failed")
                            .kind(MessageDialogKind::Error)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                }
            });
        }
        ENRICH_FNIS => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut prjct = PROJECT.lock().unwrap();
                match prjct.enrich_from_fnis_lists(&app) {
                    Ok(summary) => {
                        set_edited(true);
                        let window = app.get_webview_window(MAIN_WINDOW).unwrap();
                        emit_project_update(&window, &prjct);
                        let kind = if summary.positions_updated > 0 {
                            MessageDialogKind::Info
                        } else {
                            MessageDialogKind::Warning
                        };
                        app.dialog()
                            .message(summary.message_fnis())
                            .title("Enrich from FNIS AnimList")
                            .kind(kind)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    Err(err) => {
                        error!("{}", err);
                        app.dialog()
                            .message(&err)
                            .title("Enrich failed")
                            .kind(MessageDialogKind::Error)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                }
            });
        }
        _ => {
            error!("Unrecognized command: {}", event.id().0)
        }
    }
}

/// COMMANDS

#[tauri::command]
async fn request_project_update<R: Runtime>(window: tauri::Window<R>) -> () {
    let prjct = PROJECT.lock().unwrap();
    emit_project_update(&window, &prjct);
}

#[tauri::command]
fn set_pack_name(name: String) {
    PROJECT.lock().unwrap().pack_name = name;
}

#[tauri::command]
fn set_pack_author(author: String) {
    PROJECT.lock().unwrap().pack_author = author;
}

#[tauri::command]
async fn get_race_keys() -> Vec<String> {
    racekeys::get_race_keys_string()
}

#[tauri::command]
async fn mark_as_edited<R: Runtime>(window: tauri::Window<R>) -> () {
    set_edited(true);
    if let Ok(title) = window.title() {
        if !title.ends_with('*') {
            window.set_title(format!("{}*", title).as_str()).unwrap();
        }
    }
}

#[tauri::command]
fn get_in_darkmode() -> bool {
    get_darkmode()
}

/* Scene */

#[tauri::command]
fn create_blank_scene() -> Scene {
    Scene::default()
}

#[tauri::command]
async fn save_scene<R: Runtime>(window: tauri::Window<R>, scene: Scene) -> () {
    mark_as_edited(window).await;
    PROJECT.lock().unwrap().save_scene(scene);
}

#[tauri::command]
fn delete_scene<R: Runtime>(window: tauri::Window<R>, id: NanoID) -> Result<Scene, String> {
    let ret = PROJECT.lock().unwrap().discard_scene(&id).ok_or_else(|| {
        let msg = format!("Invalid Scene ID: {}", id.0);
        error!("{}", msg);
        msg
    });

    if ret.is_ok() {
        set_edited(true);
        if let Ok(title) = window.title() {
            if !title.ends_with('*') {
                window.set_title(format!("{}*", title).as_str()).unwrap();
            }
        }
    }

    ret
}

/* Stage */

#[derive(Debug, Serialize, Deserialize, Clone)]
struct EditorPayload {
    pub scene: NanoID,
    pub stage: Stage,
    pub positions: Vec<PositionInfo>,
    pub dark: bool,
}

fn any_stage_editor_open<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with("stage_editor_"))
}

fn set_main_window_blocked<R: Runtime>(app: &AppHandle<R>, blocked: bool) {
    let Some(main) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = main.set_enabled(!blocked);
    if !blocked {
        let _ = main.set_focus();
    }
}

fn unblock_main_if_no_stage_editors<R: Runtime>(app: &AppHandle<R>) {
    if !any_stage_editor_open(app) {
        set_main_window_blocked(app, false);
    }
}

fn open_stage_editor_impl<R: Runtime>(app: &tauri::AppHandle<R>, payload: EditorPayload) {
    let stage = &payload.stage;
    let label = format!("stage_editor_{}", stage.id.0);
    info!(
        "Opening Stage {} from Scene {}",
        stage.id.0, payload.scene.0
    );
    // Reopening the same stage: focus and re-send payload (recovers empty first open).
    if let Some(existing) = app.get_webview_window(&label) {
        set_main_window_blocked(app, true);
        let _ = existing.set_focus();
        if let Err(e) = existing.emit("on_data_received", payload.clone()) {
            error!("Failed to re-send stage editor payload: {}", e);
        }
        return;
    }
    let Some(main) = app.get_webview_window(MAIN_WINDOW) else {
        error!("Cannot open stage editor: main window missing");
        return;
    };
    let builder = WebviewWindowBuilder::new(
        app,
        label,
        tauri::WebviewUrl::App("./stage.html".into()),
    )
    .title(format!(
        "Stage Editor [{}]",
        if stage.name.is_empty() {
            "Untitled"
        } else {
            stage.name.as_str()
        }
    ))
    .min_inner_size(720.0, 540.0)
    .inner_size(1152.0, 864.0)
    .resizable(true);
    let builder = match builder.parent(&main) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to parent stage editor to main window: {}", e);
            return;
        }
    };
    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            error!(
                "Failed to create stage editor window for Stage {}: {}",
                stage.id.0, e
            );
            return;
        }
    };
    set_main_window_blocked(app, true);
    // Register before geometry restore: webview can emit before restore finishes.
    let theme_window = window.clone();
    window.clone().once("on_request_data", move |_| {
        if let Err(e) = window.emit("on_data_received", payload.clone()) {
            error!("Failed to send stage editor payload: {}", e);
        }
    });
    window_geometry::restore_window_geometry(&theme_window);
    sync_theme_from_window(&theme_window);
}

#[tauri::command]
async fn open_stage_editor<R: Runtime>(
    app: tauri::AppHandle<R>,
    active_scene: Scene,
    stage: Option<Stage>,
) -> () {
    open_stage_editor_impl(
        &app,
        EditorPayload {
            scene: active_scene.id.clone(),
            stage: stage.unwrap_or(Stage::new(&active_scene)),
            positions: active_scene.positions.clone(),
            dark: get_darkmode(),
        },
    );
}

#[tauri::command]
async fn open_stage_editor_from<R: Runtime>(
    app: tauri::AppHandle<R>,
    active_scene: Scene,
    copy_stage: Stage,
) -> () {
    // Clone must get a fresh id so save inserts a new stage instead of overwriting the source
    let mut stage = copy_stage;
    stage.id = NanoID::new_nanoid();
    if !stage.name.is_empty() {
        stage.name = format!("{} (Copy)", stage.name);
    }
    open_stage_editor_impl(
        &app,
        EditorPayload {
            scene: active_scene.id.clone(),
            stage,
            positions: active_scene.positions.clone(),
            dark: get_darkmode(),
        },
    );
}

#[tauri::command]
async fn stage_save_and_close<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    scene: NanoID,
    positions: Vec<PositionInfo>,
    stage: Stage,
) -> () {
    // IDEA: make give this event some unique id to allow
    // front end distinguish the timings at which some stage editor has been opened
    info!("Saving Stage {}", stage.id.0);
    app.emit_to(
        MAIN_WINDOW,
        "on_stage_saved",
        EditorPayload {
            scene,
            stage,
            positions,
            dark: get_darkmode(),
        },
    )
    .unwrap();
    let _ = window.close();
}

/* Position related */

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PositionPayload {
    pub position: Position,
    pub info: PositionInfo,
}

#[tauri::command]
fn make_position() -> PositionPayload {
    PositionPayload {
        position: Position::new(None),
        info: PositionInfo::default(),
    }
}
