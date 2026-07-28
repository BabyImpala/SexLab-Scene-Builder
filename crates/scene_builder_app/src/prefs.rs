use crate::theme;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePref {
    pub fn is_dark(self) -> bool {
        match self {
            ThemePref::Light => false,
            ThemePref::Dark => true,
            ThemePref::System => dark_from_env(),
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        theme::apply(ctx, self.is_dark());
    }
}

fn dark_from_env() -> bool {
    std::env::var("GTK_THEME")
        .map(|t| t.to_ascii_lowercase().contains("dark"))
        .unwrap_or(false)
        || std::env::var("COLORFGBG")
            .ok()
            .and_then(|v| v.split(';').last()?.parse::<u8>().ok())
            .map(|bg| bg < 8)
            .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    pub bottom_panel_height: f32,
    /// Height of the Furniture card inside the tags/furniture side panel.
    pub furniture_panel_height: f32,
    /// Saved custom tags ("Yours" group in the tag tree).
    pub custom_tags: Vec<String>,
    /// "Don't show this tip again on export?" (Pandora clip-folder tip).
    pub hide_export_clip_tip: bool,
    /// "Don't warn about export overwrites again?"
    pub hide_export_merge_warn: bool,
    /// Show a debug console window (Windows). Toggle under View; also `--console`.
    pub show_console: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            left_panel_width: 260.0,
            right_panel_width: 300.0,
            bottom_panel_height: 190.0,
            furniture_panel_height: 220.0,
            custom_tags: Vec::new(),
            hide_export_clip_tip: false,
            hide_export_merge_warn: false,
            show_console: false,
        }
    }
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("SexLabSceneBuilder").join("prefs.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }
}
