use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

const GEOMETRY_FILE: &str = "window_geometry.json";
const STAGE_EDITOR_KEY: &str = "stage_editor";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WindowGeometry {
    x: Option<i32>,
    y: Option<i32>,
    width: f64,
    height: f64,
    maximized: bool,
}

fn geometry_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("SexLabSceneBuilder").join(GEOMETRY_FILE))
}

fn load_store() -> HashMap<String, WindowGeometry> {
    let Some(path) = geometry_path() else {
        return HashMap::new();
    };
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn save_store(store: &HashMap<String, WindowGeometry>) {
    let Some(path) = geometry_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(store) {
        Ok(raw) => {
            if let Err(err) = fs::write(&path, raw) {
                warn!("Failed to write window geometry: {}", err);
            }
        }
        Err(err) => warn!("Failed to serialize window geometry: {}", err),
    }
}

fn storage_key(label: &str) -> &str {
    if label.starts_with("stage_editor_") {
        STAGE_EDITOR_KEY
    } else {
        label
    }
}

fn capture_geometry<R: Runtime>(window: &WebviewWindow<R>) -> Option<WindowGeometry> {
    let maximized = window.is_maximized().unwrap_or(false);
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = (size.width as f64) / scale;
    let height = (size.height as f64) / scale;
    let position = window.outer_position().ok();
    Some(WindowGeometry {
        x: position.as_ref().map(|p| p.x),
        y: position.as_ref().map(|p| p.y),
        width,
        height,
        maximized,
    })
}

pub fn save_window_geometry<R: Runtime>(window: &WebviewWindow<R>) {
    let Some(geometry) = capture_geometry(window) else {
        return;
    };
    let key = storage_key(window.label()).to_string();
    let mut store = load_store();
    store.insert(key, geometry);
    save_store(&store);
}

pub fn save_window_geometry_by_label<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        save_window_geometry(&window);
    }
}

pub fn save_all_window_geometry<R: Runtime>(app: &AppHandle<R>) {
    let mut store = load_store();
    for window in app.webview_windows().values() {
        if let Some(geometry) = capture_geometry(window) {
            store.insert(storage_key(window.label()).to_string(), geometry);
        }
    }
    save_store(&store);
}

pub fn restore_window_geometry<R: Runtime>(window: &WebviewWindow<R>) {
    let store = load_store();
    let Some(geometry) = store.get(storage_key(window.label())) else {
        return;
    };

    if geometry.width > 0.0 && geometry.height > 0.0 {
        let _ = window.set_size(tauri::LogicalSize::new(geometry.width, geometry.height));
    }
    if let (Some(x), Some(y)) = (geometry.x, geometry.y) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
    if geometry.maximized {
        let _ = window.maximize();
    }
}
