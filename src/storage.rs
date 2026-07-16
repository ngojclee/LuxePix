use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "LuxePix";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageMode {
    Installed,
    Portable,
}

#[derive(Clone, Debug)]
pub struct StoragePaths {
    pub mode: StorageMode,
    pub executable_dir: PathBuf,
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub index_dir: PathBuf,
    pub thumbnails_dir: PathBuf,
    pub updates_dir: PathBuf,
    pub similarity_dir: PathBuf,
}

impl StoragePaths {
    pub fn detect() -> Self {
        let executable = std::env::current_exe().unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("LuxePix.exe")
        });
        let executable_dir = executable
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let portable = executable_dir.join(".portable").is_file();
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        Self::from_parts(executable_dir, portable, appdata)
    }

    fn from_parts(executable_dir: PathBuf, portable: bool, appdata: Option<PathBuf>) -> Self {
        let (mode, root) = if portable {
            (StorageMode::Portable, executable_dir.join("data"))
        } else if let Some(appdata) = appdata {
            (StorageMode::Installed, appdata.join(APP_DIR_NAME))
        } else {
            (StorageMode::Portable, executable_dir.join("data"))
        };

        Self::from_root(mode, executable_dir, root)
    }

    pub(crate) fn from_root(mode: StorageMode, executable_dir: PathBuf, root: PathBuf) -> Self {
        Self {
            mode,
            executable_dir,
            config: root.join("config.json"),
            state: root.join("state.json"),
            index_dir: root.join("index"),
            thumbnails_dir: root.join("thumbnails"),
            updates_dir: root.join("updates"),
            similarity_dir: root.join("similarity"),
            root,
        }
    }

    pub fn ensure(&self) -> io::Result<()> {
        for directory in [
            &self.root,
            &self.index_dir,
            &self.thumbnails_dir,
            &self.updates_dir,
            &self.similarity_dir,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(())
    }

    pub fn legacy_config_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = vec![self.executable_dir.join("luxepix_config.json")];
        let appdata_legacy = self.root.join("luxepix_config.json");
        if !candidates.contains(&appdata_legacy) {
            candidates.push(appdata_legacy);
        }
        candidates
    }

    pub const fn mode_label(&self) -> &'static str {
        match self.mode {
            StorageMode::Installed => "Installed",
            StorageMode::Portable => "Portable",
        }
    }
}

pub fn transactional_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("state"));
    let mut temp_name = OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp_path = parent.join(temp_name);
    let backup_path = path.with_extension("bak");

    let _ = fs::remove_file(&temp_path);
    let _ = fs::remove_file(&backup_path);

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    let had_previous = path.exists();
    if had_previous {
        fs::rename(path, &backup_path)?;
    }

    match fs::rename(&temp_path, path) {
        Ok(()) => {
            if had_previous {
                let _ = fs::remove_file(backup_path);
            }
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            if had_previous && backup_path.exists() {
                let _ = fs::rename(&backup_path, path);
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{transactional_write, StorageMode, StoragePaths};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("luxepix-{name}-{}-{unique}", std::process::id()))
    }

    #[test]
    fn portable_layout_keeps_mutable_data_beside_the_executable() {
        let executable_dir = PathBuf::from(r"C:\Apps\LuxePix");
        let paths =
            StoragePaths::from_parts(executable_dir.clone(), true, Some(PathBuf::from("ignored")));

        assert_eq!(paths.mode, StorageMode::Portable);
        assert_eq!(paths.root, executable_dir.join("data"));
        assert_eq!(
            paths.config,
            executable_dir.join("data").join("config.json")
        );
    }

    #[test]
    fn installed_layout_uses_appdata() {
        let paths = StoragePaths::from_parts(
            PathBuf::from(r"C:\Program Files\LuxePix"),
            false,
            Some(PathBuf::from(r"C:\Users\Me\AppData\Roaming")),
        );

        assert_eq!(paths.mode, StorageMode::Installed);
        assert_eq!(
            paths.root,
            PathBuf::from(r"C:\Users\Me\AppData\Roaming\LuxePix")
        );
    }

    #[test]
    fn transactional_write_replaces_existing_content() {
        let root = test_root("transactional-write");
        let path = root.join("config.json");
        fs::create_dir_all(&root).expect("create test root");
        fs::write(&path, b"old").expect("write old content");

        transactional_write(&path, b"new").expect("replace content");

        assert_eq!(fs::read(&path).expect("read new content"), b"new");
        assert!(!path.with_extension("bak").exists());
        fs::remove_dir_all(root).expect("remove test root");
    }
}
