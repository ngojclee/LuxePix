use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::model::{FileEntry, FolderEntry, ItemId};

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff", "tif", "ico", "avif",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    pub show_hidden_files: bool,
}

impl ScanOptions {
    pub const fn new(show_hidden_files: bool) -> Self {
        Self { show_hidden_files }
    }
}

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan a directory and return info about each sub-folder (first image + count).
pub fn scan_directory(path: &Path, options: ScanOptions) -> io::Result<Vec<FolderEntry>> {
    let mut folders = Vec::new();

    let entries = fs::read_dir(path)?;

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|entry| entry_is_visible(entry, options.show_hidden_files))
        .filter(|entry| {
            entry
                .file_type()
                .ok()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();

    dirs.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    for dir in dirs {
        let all = list_all_files(&dir, options).unwrap_or_default();
        let first_img = all.iter().find(|f| f.is_image).map(|f| f.path.clone());
        let img_count = all.iter().filter(|f| f.is_image).count();
        let total = all.len();
        let size_bytes = all.iter().map(|file| file.size_bytes).sum();
        let modified_unix = fs::metadata(&dir)
            .ok()
            .as_ref()
            .map(metadata_modified_unix)
            .unwrap_or(0);
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        folders.push(FolderEntry {
            id: ItemId::from(dir.as_path()),
            path: dir,
            name,
            first_image: first_img,
            image_count: img_count,
            total_files: total,
            size_bytes,
            modified_unix,
        });
    }

    Ok(folders)
}

/// List ALL files in a directory (non-recursive), sorted naturally.
/// Images come first, then other files.
pub fn list_all_files(path: &Path, options: ScanOptions) -> io::Result<Vec<FileEntry>> {
    let mut files = Vec::new();

    let entries = fs::read_dir(path)?;
    for entry in entries.filter_map(Result::ok) {
        if !entry_is_visible(&entry, options.show_hidden_files) {
            continue;
        }
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ext = p
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let metadata = entry.metadata().ok();
        let size = metadata
            .as_ref()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let modified_unix = metadata.as_ref().map(metadata_modified_unix).unwrap_or(0);

        files.push(FileEntry {
            id: ItemId::from(p.as_path()),
            is_image: is_image_file(&p),
            path: p,
            name,
            extension: ext,
            size_bytes: size,
            modified_unix,
        });
    }

    // Sort: images first, then by name naturally
    files.sort_by(|a, b| {
        b.is_image
            .cmp(&a.is_image)
            .then_with(|| natord::compare(&a.name, &b.name))
    });

    Ok(files)
}

fn metadata_modified_unix(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn entry_is_visible(entry: &fs::DirEntry, show_hidden_files: bool) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if !show_hidden_files && name.starts_with('.') {
        return false;
    }

    let Ok(metadata) = entry.metadata() else {
        return true;
    };

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        let attributes = metadata.file_attributes();
        if attributes & FILE_ATTRIBUTE_SYSTEM != 0 {
            return false;
        }
        if !show_hidden_files && attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
            return false;
        }
    }

    true
}

/// Format bytes to human-readable size string.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, is_image_file, list_all_files, ScanOptions};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn recognizes_image_extensions_case_insensitively() {
        assert!(is_image_file(Path::new("sample.JPEG")));
        assert!(is_image_file(Path::new("preview.webp")));
        assert!(!is_image_file(Path::new("notes.txt")));
        assert!(!is_image_file(Path::new("no-extension")));
    }

    #[test]
    fn formats_byte_sizes_at_unit_boundaries() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn dot_prefixed_files_follow_the_hidden_visibility_option() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("luxepix-hidden-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).expect("create scan fixture");
        fs::write(root.join(".hidden.jpg"), b"hidden").expect("write hidden fixture");
        fs::write(root.join("visible.jpg"), b"visible").expect("write visible fixture");

        let hidden_off =
            list_all_files(&root, ScanOptions::new(false)).expect("scan without hidden files");
        let hidden_on =
            list_all_files(&root, ScanOptions::new(true)).expect("scan with hidden files");

        assert_eq!(hidden_off.len(), 1);
        assert_eq!(hidden_on.len(), 2);
        fs::remove_dir_all(root).expect("remove scan fixture");
    }
}
