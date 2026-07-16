use std::path::{Path, PathBuf};

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

#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub id: ItemId,
    pub path: PathBuf,
    pub name: String,
    pub first_image: Option<PathBuf>,
    pub image_count: usize,
    pub total_files: usize,
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub id: ItemId,
    pub path: PathBuf,
    pub name: String,
    pub is_image: bool,
    pub extension: String,
    pub size_bytes: u64,
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
