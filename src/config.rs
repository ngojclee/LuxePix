use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::CommandId;
use crate::model::{SortDirection, SortField, ViewMode};
use crate::storage::{transactional_write, StoragePaths};

const CURRENT_SCHEMA_VERSION: u32 = 5;
const MAX_HISTORY: usize = 50;
const SHORTCUT_PROFILE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub path_history: Vec<String>,
    pub left_thumb_size: f32,
    pub right_thumb_size: f32,
    pub left_panel_width: f32,
    pub last_path: Option<String>,
    pub theme: String,
    pub shortcuts: ShortcutConfig,
    pub sources: Vec<SourceConfig>,
    pub show_hidden_files: bool,
    pub show_selection_checkboxes: bool,
    pub confirm_permanent_delete: bool,
    pub confirm_recycle_delete: bool,
    pub check_updates_on_startup: bool,
    pub indexing_enabled: bool,
    pub similarity_result_count: usize,
    pub sidebar_collapsed: bool,
    pub details_panel_open: bool,
    pub file_view_mode: ViewMode,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub first_run_complete: bool,
    #[serde(skip)]
    config_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub name: String,
    pub path: String,
    pub favorite: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    pub copy: String,
    pub cut: String,
    pub paste: String,
    pub rename: String,
    pub delete: String,
    pub delete_permanent: String,
    pub open: String,
    pub new_folder: String,
    pub select_all: String,
    pub undo: String,
    pub refresh: String,
    pub find: String,
    pub back: String,
    pub forward: String,
    pub up: String,
    pub preview: String,
    pub settings: String,
    pub open_with: String,
    pub reveal_in_explorer: String,
    pub edit_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ShortcutProfile {
    schema_version: u32,
    shortcuts: ShortcutConfig,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            copy: CommandId::Copy.default_shortcut().into(),
            cut: CommandId::Cut.default_shortcut().into(),
            paste: CommandId::Paste.default_shortcut().into(),
            rename: CommandId::Rename.default_shortcut().into(),
            delete: CommandId::Delete.default_shortcut().into(),
            delete_permanent: CommandId::DeletePermanent.default_shortcut().into(),
            open: CommandId::Open.default_shortcut().into(),
            new_folder: CommandId::NewFolder.default_shortcut().into(),
            select_all: CommandId::SelectAll.default_shortcut().into(),
            undo: CommandId::Undo.default_shortcut().into(),
            refresh: CommandId::Refresh.default_shortcut().into(),
            find: CommandId::Find.default_shortcut().into(),
            back: CommandId::Back.default_shortcut().into(),
            forward: CommandId::Forward.default_shortcut().into(),
            up: CommandId::Up.default_shortcut().into(),
            preview: CommandId::Preview.default_shortcut().into(),
            settings: CommandId::Settings.default_shortcut().into(),
            open_with: CommandId::OpenWith.default_shortcut().into(),
            reveal_in_explorer: CommandId::RevealInExplorer.default_shortcut().into(),
            edit_address: CommandId::EditAddress.default_shortcut().into(),
        }
    }
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            name: "Products".into(),
            path: String::new(),
            favorite: false,
        }
    }
}

impl ShortcutConfig {
    pub fn get(&self, command: CommandId) -> &str {
        match command {
            CommandId::Copy => &self.copy,
            CommandId::Cut => &self.cut,
            CommandId::Paste => &self.paste,
            CommandId::Rename => &self.rename,
            CommandId::Delete => &self.delete,
            CommandId::DeletePermanent => &self.delete_permanent,
            CommandId::Open => &self.open,
            CommandId::NewFolder => &self.new_folder,
            CommandId::SelectAll => &self.select_all,
            CommandId::Undo => &self.undo,
            CommandId::Refresh => &self.refresh,
            CommandId::Find => &self.find,
            CommandId::Back => &self.back,
            CommandId::Forward => &self.forward,
            CommandId::Up => &self.up,
            CommandId::Preview => &self.preview,
            CommandId::Settings => &self.settings,
            CommandId::OpenWith => &self.open_with,
            CommandId::RevealInExplorer => &self.reveal_in_explorer,
            CommandId::EditAddress => &self.edit_address,
        }
    }

    pub fn get_mut(&mut self, command: CommandId) -> &mut String {
        match command {
            CommandId::Copy => &mut self.copy,
            CommandId::Cut => &mut self.cut,
            CommandId::Paste => &mut self.paste,
            CommandId::Rename => &mut self.rename,
            CommandId::Delete => &mut self.delete,
            CommandId::DeletePermanent => &mut self.delete_permanent,
            CommandId::Open => &mut self.open,
            CommandId::NewFolder => &mut self.new_folder,
            CommandId::SelectAll => &mut self.select_all,
            CommandId::Undo => &mut self.undo,
            CommandId::Refresh => &mut self.refresh,
            CommandId::Find => &mut self.find,
            CommandId::Back => &mut self.back,
            CommandId::Forward => &mut self.forward,
            CommandId::Up => &mut self.up,
            CommandId::Preview => &mut self.preview,
            CommandId::Settings => &mut self.settings,
            CommandId::OpenWith => &mut self.open_with,
            CommandId::RevealInExplorer => &mut self.reveal_in_explorer,
            CommandId::EditAddress => &mut self.edit_address,
        }
    }

    pub fn conflicts(&self) -> Vec<(CommandId, CommandId, String)> {
        let mut conflicts = Vec::new();
        for (index, left) in CommandId::CONFIGURABLE.iter().copied().enumerate() {
            let left_value = normalize_shortcut(self.get(left));
            if left_value.is_empty() {
                continue;
            }
            for right in CommandId::CONFIGURABLE.iter().copied().skip(index + 1) {
                if left_value == normalize_shortcut(self.get(right)) {
                    conflicts.push((left, right, self.get(left).trim().to_string()));
                }
            }
        }
        conflicts
    }

    pub fn invalid_commands(&self) -> Vec<CommandId> {
        CommandId::CONFIGURABLE
            .iter()
            .copied()
            .filter(|command| !is_valid_shortcut(self.get(*command)))
            .collect()
    }

    pub fn validation_error(&self) -> Option<String> {
        if let Some(command) = self.invalid_commands().first() {
            return Some(format!(
                "{} has an invalid shortcut: {}.",
                command.label(),
                self.get(*command).trim()
            ));
        }
        if let Some((left, right, shortcut)) = self.conflicts().first() {
            return Some(format!(
                "{} is assigned to both {} and {}.",
                shortcut,
                left.label(),
                right.label()
            ));
        }
        None
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            path_history: Vec::new(),
            left_thumb_size: 160.0,
            right_thumb_size: 180.0,
            left_panel_width: 420.0,
            last_path: None,
            theme: "system".into(),
            shortcuts: ShortcutConfig::default(),
            sources: Vec::new(),
            show_hidden_files: false,
            show_selection_checkboxes: false,
            confirm_permanent_delete: true,
            confirm_recycle_delete: false,
            check_updates_on_startup: true,
            indexing_enabled: true,
            similarity_result_count: 10,
            sidebar_collapsed: false,
            details_panel_open: true,
            file_view_mode: ViewMode::Gallery,
            sort_field: SortField::Name,
            sort_direction: SortDirection::Ascending,
            first_run_complete: false,
            config_path: PathBuf::new(),
        }
    }
}

impl AppConfig {
    pub fn load(paths: &StoragePaths) -> Self {
        let _ = paths.ensure();
        let mut candidates = vec![paths.config.clone()];
        candidates.extend(paths.legacy_config_candidates());

        for candidate in candidates {
            let Ok(data) = fs::read_to_string(&candidate) else {
                continue;
            };
            let Ok(mut config) = serde_json::from_str::<Self>(&data) else {
                continue;
            };
            config.config_path = paths.config.clone();
            config.normalize();
            if candidate != paths.config {
                let _ = config.save();
            }
            return config;
        }

        Self {
            config_path: paths.config.clone(),
            ..Self::default()
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        transactional_write(&self.config_path, &json)
    }

    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme.as_str() {
            "light" => "dark",
            "dark" => "system",
            _ => "light",
        }
        .into();
        let _ = self.save();
    }

    pub fn add_path(&mut self, path: &str) {
        let path = path.to_string();
        self.path_history.retain(|existing| existing != &path);
        self.path_history.insert(0, path.clone());
        self.path_history.truncate(MAX_HISTORY);
        self.last_path = Some(path);
        let _ = self.save();
    }

    pub fn remove_path(&mut self, path: &str) {
        self.path_history.retain(|existing| existing != path);
        let _ = self.save();
    }

    pub fn add_source(&mut self, path: &Path) {
        let path_value = path.to_string_lossy().to_string();
        if self
            .sources
            .iter()
            .any(|source| source.path.eq_ignore_ascii_case(&path_value))
        {
            return;
        }
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path_value.clone());
        self.sources.push(SourceConfig {
            name,
            path: path_value,
            favorite: self.sources.is_empty(),
        });
        let _ = self.save();
    }

    pub fn remove_source(&mut self, path: &str) {
        self.sources.retain(|source| source.path != path);
        let _ = self.save();
    }

    pub fn toggle_source_favorite(&mut self, path: &str) {
        if let Some(source) = self.sources.iter_mut().find(|source| source.path == path) {
            source.favorite = !source.favorite;
            let _ = self.save();
        }
    }

    pub fn save_ui_state(&mut self, left_thumb: f32, right_thumb: f32, panel_width: f32) {
        let changed = (self.left_thumb_size - left_thumb).abs() > 0.5
            || (self.right_thumb_size - right_thumb).abs() > 0.5
            || (self.left_panel_width - panel_width).abs() > 1.0;
        if changed {
            self.left_thumb_size = left_thumb;
            self.right_thumb_size = right_thumb;
            self.left_panel_width = panel_width;
            let _ = self.save();
        }
    }

    pub fn export_shortcut_profile(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&ShortcutProfile {
            schema_version: SHORTCUT_PROFILE_VERSION,
            shortcuts: self.shortcuts.clone(),
        })
    }

    pub fn import_shortcut_profile(&mut self, json: &str) -> Result<(), String> {
        let profile: ShortcutProfile =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        if profile.schema_version != SHORTCUT_PROFILE_VERSION {
            return Err(format!(
                "Unsupported shortcut profile version {}.",
                profile.schema_version
            ));
        }
        if let Some(error) = profile.shortcuts.validation_error() {
            return Err(error);
        }
        self.shortcuts = profile.shortcuts;
        self.save().map_err(|error| error.to_string())
    }

    fn normalize(&mut self) {
        self.schema_version = CURRENT_SCHEMA_VERSION;
        self.left_thumb_size = self.left_thumb_size.clamp(80.0, 500.0);
        self.right_thumb_size = self.right_thumb_size.clamp(80.0, 500.0);
        self.left_panel_width = self.left_panel_width.clamp(280.0, 760.0);
        if !matches!(self.theme.as_str(), "light" | "dark" | "system") {
            self.theme = "system".into();
        }
        self.similarity_result_count = self.similarity_result_count.clamp(5, 50);
        self.confirm_permanent_delete = true;
        self.path_history.truncate(MAX_HISTORY);
        for source in &mut self.sources {
            source.path = source.path.trim().to_string();
            source.name = source.name.trim().to_string();
            if source.name.is_empty() {
                source.name = Path::new(&source.path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.path.clone());
            }
        }
        let mut seen_sources = HashSet::new();
        self.sources.retain(|source| {
            !source.path.is_empty()
                && seen_sources.insert(source.path.replace('/', "\\").to_ascii_lowercase())
        });
    }
}

fn normalize_shortcut(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_valid_shortcut(value: &str) -> bool {
    let parts: Vec<String> = value
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return true;
    }

    let mut key_count = 0usize;
    for part in parts {
        if matches!(part.as_str(), "ctrl" | "control" | "shift" | "alt") {
            continue;
        }
        if !is_supported_shortcut_key(&part) {
            return false;
        }
        key_count += 1;
    }
    key_count == 1
}

fn is_supported_shortcut_key(value: &str) -> bool {
    matches!(
        value,
        "a" | "b"
            | "c"
            | "d"
            | "e"
            | "f"
            | "g"
            | "h"
            | "i"
            | "j"
            | "k"
            | "l"
            | "m"
            | "n"
            | "o"
            | "p"
            | "q"
            | "r"
            | "s"
            | "t"
            | "u"
            | "v"
            | "w"
            | "x"
            | "y"
            | "z"
            | "0"
            | "1"
            | "2"
            | "3"
            | "4"
            | "5"
            | "6"
            | "7"
            | "8"
            | "9"
            | "enter"
            | "return"
            | "delete"
            | "del"
            | "backspace"
            | "escape"
            | "esc"
            | "space"
            | "tab"
            | "insert"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "left"
            | "right"
            | "up"
            | "down"
            | "f1"
            | "f2"
            | "f3"
            | "f4"
            | "f5"
            | "f6"
            | "f7"
            | "f8"
            | "f9"
            | "f10"
            | "f11"
            | "f12"
            | ","
            | "comma"
    )
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, ShortcutConfig};
    use crate::commands::CommandId;
    use crate::storage::{StorageMode, StoragePaths};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_paths(name: &str) -> StoragePaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "luxepix-config-{name}-{}-{unique}",
            std::process::id()
        ));
        StoragePaths::from_root(StorageMode::Portable, root.clone(), root.join("data"))
    }

    #[test]
    fn detects_shortcut_conflicts_case_insensitively() {
        let shortcuts = ShortcutConfig {
            paste: " ctrl+c ".into(),
            ..ShortcutConfig::default()
        };
        let conflicts = shortcuts.conflicts();

        assert!(conflicts
            .iter()
            .any(|(left, right, _)| *left == CommandId::Copy && *right == CommandId::Paste));
    }

    #[test]
    fn saves_and_loads_typed_config() {
        let paths = test_paths("roundtrip");
        let mut config = AppConfig::load(&paths);
        config.theme = "dark".into();
        config.similarity_result_count = 23;
        config.add_path(r"N:\Products");

        let loaded = AppConfig::load(&paths);

        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.similarity_result_count, 23);
        assert_eq!(loaded.last_path.as_deref(), Some(r"N:\Products"));
        fs::remove_dir_all(paths.executable_dir).expect("remove test root");
    }

    #[test]
    fn shortcut_profile_rejects_conflicts() {
        let paths = test_paths("shortcut-profile");
        let mut config = AppConfig::load(&paths);
        let defaults = ShortcutConfig::default();
        let conflicting = ShortcutConfig {
            cut: defaults.copy.clone(),
            ..defaults
        };
        let json = serde_json::json!({
            "schema_version": 1,
            "shortcuts": conflicting,
        })
        .to_string();

        assert!(config.import_shortcut_profile(&json).is_err());
        fs::remove_dir_all(paths.executable_dir).expect("remove test root");
    }

    #[test]
    fn shortcut_validation_rejects_unknown_keys() {
        let shortcuts = ShortcutConfig {
            copy: "Ctrl+DefinitelyNotAKey".into(),
            ..ShortcutConfig::default()
        };

        assert_eq!(
            shortcuts.invalid_commands(),
            vec![CommandId::Copy],
            "invalid shortcut should identify its command"
        );
    }

    #[test]
    fn migrates_older_config_and_normalizes_sources() {
        let paths = test_paths("migration");
        paths.ensure().expect("create config directories");
        let json = serde_json::json!({
            "schema_version": 2,
            "theme": "unknown",
            "similarity_result_count": 500,
            "sources": [
                {"name": "", "path": "  N:/Products  ", "favorite": true},
                {"name": "Duplicate", "path": "n:\\products", "favorite": false},
                {"name": "Empty", "path": " ", "favorite": false}
            ]
        });
        fs::write(
            &paths.config,
            serde_json::to_vec_pretty(&json).expect("serialize legacy config"),
        )
        .expect("write legacy config");

        let loaded = AppConfig::load(&paths);

        assert_eq!(loaded.schema_version, 5);
        assert_eq!(loaded.theme, "system");
        assert_eq!(loaded.similarity_result_count, 50);
        assert_eq!(loaded.sources.len(), 1);
        assert_eq!(loaded.sources[0].name, "Products");
        assert_eq!(loaded.sources[0].path, "N:/Products");
        fs::remove_dir_all(paths.executable_dir).expect("remove test root");
    }

    #[test]
    fn source_changes_are_persisted() {
        let paths = test_paths("sources");
        let source_path = paths.executable_dir.join("Products");
        fs::create_dir_all(&source_path).expect("create source folder");
        let mut config = AppConfig::load(&paths);

        config.add_source(&source_path);
        assert_eq!(config.sources.len(), 1);
        assert!(config.sources[0].favorite);

        let source_value = source_path.to_string_lossy().to_string();
        config.toggle_source_favorite(&source_value);
        let loaded = AppConfig::load(&paths);
        assert_eq!(loaded.sources.len(), 1);
        assert!(!loaded.sources[0].favorite);

        config.remove_source(&source_value);
        let loaded = AppConfig::load(&paths);
        assert!(loaded.sources.is_empty());
        fs::remove_dir_all(paths.executable_dir).expect("remove test root");
    }
}
