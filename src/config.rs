use std::fs;
use std::path::PathBuf;

const MAX_HISTORY: usize = 50;
const CONFIG_FILE: &str = "luxepix_config.json";

pub struct AppConfig {
    pub path_history: Vec<String>,
    pub left_thumb_size: f32,
    pub right_thumb_size: f32,
    pub left_panel_width: f32,
    pub last_path: Option<String>,
    pub theme: String,
    pub shortcuts: ShortcutConfig,
}

pub struct ShortcutConfig {
    pub copy: String,
    pub cut: String,
    pub paste: String,
    pub rename: String,
    pub delete: String,
    pub open: String,
    pub settings: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            copy: "Ctrl+C".into(),
            cut: "Ctrl+X".into(),
            paste: "Ctrl+V".into(),
            rename: "F2".into(),
            delete: "Delete".into(),
            open: "Enter".into(),
            settings: "Ctrl+,".into(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            path_history: Vec::new(),
            left_thumb_size: 160.0,
            right_thumb_size: 180.0,
            left_panel_width: 420.0,
            last_path: None,
            theme: "light".into(),
            shortcuts: ShortcutConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load config from disk.
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                let history = json
                    .get("path_history")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let left_thumb = json.get("left_thumb_size")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(160.0) as f32;
                let right_thumb = json.get("right_thumb_size")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(180.0) as f32;
                let panel_w = json.get("left_panel_width")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(420.0) as f32;
                let last = json.get("last_path")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let theme = json.get("theme")
                    .and_then(|v| v.as_str())
                    .filter(|v| *v == "light" || *v == "dark")
                    .unwrap_or("light")
                    .to_string();
                let shortcuts_json = json.get("shortcuts");
                let defaults = ShortcutConfig::default();
                let shortcuts = ShortcutConfig {
                    copy: read_shortcut(shortcuts_json, "copy", &defaults.copy),
                    cut: read_shortcut(shortcuts_json, "cut", &defaults.cut),
                    paste: read_shortcut(shortcuts_json, "paste", &defaults.paste),
                    rename: read_shortcut(shortcuts_json, "rename", &defaults.rename),
                    delete: read_shortcut(shortcuts_json, "delete", &defaults.delete),
                    open: read_shortcut(shortcuts_json, "open", &defaults.open),
                    settings: read_shortcut(shortcuts_json, "settings", &defaults.settings),
                };

                return AppConfig {
                    path_history: history,
                    left_thumb_size: left_thumb,
                    right_thumb_size: right_thumb,
                    left_panel_width: panel_w,
                    last_path: last,
                    theme,
                    shortcuts,
                };
            }
        }
        AppConfig::default()
    }

    /// Save config to disk.
    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::json!({
            "path_history": self.path_history,
            "left_thumb_size": self.left_thumb_size,
            "right_thumb_size": self.right_thumb_size,
            "left_panel_width": self.left_panel_width,
            "last_path": self.last_path,
            "theme": self.theme,
            "shortcuts": {
                "copy": self.shortcuts.copy,
                "cut": self.shortcuts.cut,
                "paste": self.shortcuts.paste,
                "rename": self.shortcuts.rename,
                "delete": self.shortcuts.delete,
                "open": self.shortcuts.open,
                "settings": self.shortcuts.settings,
            },
        });
        let _ = fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
    }

    pub fn toggle_theme(&mut self) {
        self.theme = if self.theme == "dark" { "light" } else { "dark" }.into();
        self.save();
    }

    /// Add a path to history (most recent first, deduplicate, cap at MAX_HISTORY).
    pub fn add_path(&mut self, path: &str) {
        let path = path.to_string();
        self.path_history.retain(|p| p != &path);
        self.path_history.insert(0, path.clone());
        self.path_history.truncate(MAX_HISTORY);
        self.last_path = Some(path);
        self.save();
    }

    /// Remove a specific path from history.
    pub fn remove_path(&mut self, path: &str) {
        self.path_history.retain(|p| p != path);
        self.save();
    }

    /// Save UI state only if changed.
    pub fn save_ui_state(&mut self, left_thumb: f32, right_thumb: f32, panel_width: f32) {
        let changed = (self.left_thumb_size - left_thumb).abs() > 0.5
            || (self.right_thumb_size - right_thumb).abs() > 0.5
            || (self.left_panel_width - panel_width).abs() > 1.0;
        if changed {
            self.left_thumb_size = left_thumb;
            self.right_thumb_size = right_thumb;
            self.left_panel_width = panel_width;
            self.save();
        }
    }
}

fn read_shortcut(json: Option<&serde_json::Value>, key: &str, fallback: &str) -> String {
    json.and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn config_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let local_config = dir.join(CONFIG_FILE);
            let portable_marker = dir.join(".portable");
            if local_config.exists() || portable_marker.exists() {
                return local_config;
            }
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("LuxePix").join(CONFIG_FILE);
    }
    std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .unwrap_or(&PathBuf::from("."))
        .join(CONFIG_FILE)
}
