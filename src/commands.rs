use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandContext {
    pub has_selection: bool,
    pub has_clipboard: bool,
    pub has_target_directory: bool,
    pub has_current_path: bool,
    pub has_items: bool,
    pub has_previewable_selection: bool,
    pub can_navigate_back: bool,
    pub can_navigate_forward: bool,
    pub can_navigate_up: bool,
    pub can_undo: bool,
    pub has_single_selection: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    Copy,
    Cut,
    Paste,
    Rename,
    Delete,
    DeletePermanent,
    Open,
    NewFolder,
    SelectAll,
    Undo,
    Refresh,
    Find,
    Back,
    Forward,
    Up,
    Preview,
    Settings,
    OpenWith,
    RevealInExplorer,
    EditAddress,
}

impl CommandId {
    pub const CONFIGURABLE: [Self; 20] = [
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::Rename,
        Self::Delete,
        Self::DeletePermanent,
        Self::Open,
        Self::NewFolder,
        Self::SelectAll,
        Self::Undo,
        Self::Refresh,
        Self::Find,
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Preview,
        Self::Settings,
        Self::OpenWith,
        Self::RevealInExplorer,
        Self::EditAddress,
    ];

    pub const ACTIVE: [Self; 19] = [
        Self::Settings,
        Self::EditAddress,
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::NewFolder,
        Self::Rename,
        Self::Delete,
        Self::DeletePermanent,
        Self::Open,
        Self::OpenWith,
        Self::RevealInExplorer,
        Self::SelectAll,
        Self::Undo,
        Self::Refresh,
        Self::Preview,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Cut => "Cut",
            Self::Paste => "Paste",
            Self::Rename => "Rename",
            Self::Delete => "Delete to Recycle Bin",
            Self::DeletePermanent => "Delete Permanently",
            Self::Open => "Open",
            Self::NewFolder => "New Folder",
            Self::SelectAll => "Select All",
            Self::Undo => "Undo",
            Self::Refresh => "Refresh",
            Self::Find => "Quick Find",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Up => "Up",
            Self::Preview => "Preview",
            Self::Settings => "Settings",
            Self::OpenWith => "Open With",
            Self::RevealInExplorer => "Reveal in Windows Explorer",
            Self::EditAddress => "Focus Address Bar",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Delete => "Delete",
            Self::DeletePermanent => "Delete Permanently",
            Self::Find => "Find",
            Self::Preview => "Preview",
            Self::RevealInExplorer => "Reveal in Explorer",
            Self::EditAddress => "Address Bar",
            _ => self.label(),
        }
    }

    pub const fn default_shortcut(self) -> &'static str {
        match self {
            Self::Copy => "Ctrl+C",
            Self::Cut => "Ctrl+X",
            Self::Paste => "Ctrl+V",
            Self::Rename => "F2",
            Self::Delete => "Delete",
            Self::DeletePermanent => "Shift+Delete",
            Self::Open => "Enter",
            Self::NewFolder => "Ctrl+Shift+N",
            Self::SelectAll => "Ctrl+A",
            Self::Undo => "Ctrl+Z",
            Self::Refresh => "F5",
            Self::Find => "Ctrl+F",
            Self::Back => "Alt+Left",
            Self::Forward => "Alt+Right",
            Self::Up => "Alt+Up",
            Self::Preview => "Space",
            Self::Settings => "Ctrl+,",
            Self::OpenWith => "Shift+Enter",
            Self::RevealInExplorer => "Ctrl+Shift+E",
            Self::EditAddress => "Ctrl+L",
        }
    }

    pub const fn enabled(self, context: CommandContext) -> bool {
        match self {
            Self::Copy | Self::Cut | Self::Delete | Self::DeletePermanent => context.has_selection,
            Self::Rename | Self::Open | Self::OpenWith | Self::RevealInExplorer => {
                context.has_single_selection
            }
            Self::Paste => context.has_clipboard && context.has_target_directory,
            Self::NewFolder | Self::Refresh | Self::Find => context.has_current_path,
            Self::SelectAll => context.has_items,
            Self::Undo => context.can_undo,
            Self::Back => context.can_navigate_back,
            Self::Forward => context.can_navigate_forward,
            Self::Up => context.can_navigate_up,
            Self::Preview => context.has_previewable_selection,
            Self::Settings | Self::EditAddress => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandContext, CommandId};
    use std::collections::HashSet;

    #[test]
    fn configurable_commands_have_unique_default_shortcuts() {
        let mut seen = HashSet::new();
        for command in CommandId::CONFIGURABLE {
            assert!(seen.insert(command.default_shortcut().to_ascii_lowercase()));
        }
    }

    #[test]
    fn active_commands_are_configurable() {
        for command in CommandId::ACTIVE {
            assert!(CommandId::CONFIGURABLE.contains(&command));
        }
    }

    #[test]
    fn command_enablement_uses_one_context_contract() {
        let empty = CommandContext::default();
        assert!(!CommandId::Copy.enabled(empty));
        assert!(!CommandId::Paste.enabled(empty));
        assert!(CommandId::Settings.enabled(empty));

        let selected = CommandContext {
            has_selection: true,
            has_single_selection: true,
            has_previewable_selection: true,
            ..empty
        };
        assert!(CommandId::Copy.enabled(selected));
        assert!(CommandId::Open.enabled(selected));
        assert!(CommandId::Rename.enabled(selected));
        assert!(CommandId::Preview.enabled(selected));

        let multi_selected = CommandContext {
            has_selection: true,
            has_single_selection: false,
            ..empty
        };
        assert!(CommandId::Copy.enabled(multi_selected));
        assert!(!CommandId::Open.enabled(multi_selected));
        assert!(!CommandId::Rename.enabled(multi_selected));

        let clipboard_without_target = CommandContext {
            has_clipboard: true,
            ..empty
        };
        assert!(!CommandId::Paste.enabled(clipboard_without_target));

        let paste_ready = CommandContext {
            has_clipboard: true,
            has_target_directory: true,
            ..empty
        };
        assert!(CommandId::Paste.enabled(paste_ready));
    }
}
