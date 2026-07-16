use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ItemId(PathBuf);

impl ItemId {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for ItemId {
    fn from(path: PathBuf) -> Self {
        Self::from_path(path)
    }
}

impl From<&Path> for ItemId {
    fn from(path: &Path) -> Self {
        Self::from_path(path)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    Gallery,
    Grid,
    Compact,
    Details,
}

impl ViewMode {
    pub const ALL: [Self; 4] = [Self::Gallery, Self::Grid, Self::Compact, Self::Details];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Gallery => "Gallery",
            Self::Grid => "Grid",
            Self::Compact => "Compact",
            Self::Details => "Details",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    #[default]
    Name,
    Modified,
    Size,
    Type,
}

impl SortField {
    pub const ALL: [Self; 4] = [Self::Name, Self::Modified, Self::Size, Self::Type];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Modified => "Modified",
            Self::Size => "Size",
            Self::Type => "Type",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub id: ItemId,
    pub path: PathBuf,
    pub name: String,
    pub first_image: Option<PathBuf>,
    pub image_count: usize,
    pub total_files: usize,
    pub size_bytes: u64,
    pub modified_unix: u64,
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub id: ItemId,
    pub path: PathBuf,
    pub name: String,
    pub is_image: bool,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_unix: u64,
}

#[cfg(test)]
mod tests {
    use super::ItemId;
    use std::path::Path;

    #[test]
    fn item_id_is_stable_for_the_same_path() {
        let left = ItemId::from(Path::new(r"N:\Products\1234"));
        let right = ItemId::from(Path::new(r"N:\Products\1234"));

        assert_eq!(left, right);
        assert_eq!(left.as_path(), Path::new(r"N:\Products\1234"));
    }
}
