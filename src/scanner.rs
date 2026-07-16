use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{FileEntry, FolderEntry, ItemId};

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff", "tif", "ico", "avif",
];

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan a directory and return info about each sub-folder (first image + count).
pub fn scan_directory(path: &Path) -> io::Result<Vec<FolderEntry>> {
    let mut folders = Vec::new();

    let entries = fs::read_dir(path)?;

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().ok().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();

    dirs.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    for dir in dirs {
        let all = list_all_files(&dir).unwrap_or_default();
        let first_img = all.iter().find(|f| f.is_image).map(|f| f.path.clone());
        let img_count = all.iter().filter(|f| f.is_image).count();
        let total = all.len();
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
        });
    }

    Ok(folders)
}

/// List ALL files in a directory (non-recursive), sorted naturally.
/// Images come first, then other files.
pub fn list_all_files(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut files = Vec::new();

    let entries = fs::read_dir(path)?;
    for entry in entries.filter_map(Result::ok) {
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
        let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);

        files.push(FileEntry {
            id: ItemId::from(p.as_path()),
            is_image: is_image_file(&p),
            path: p,
            name,
            extension: ext,
            size_bytes: size,
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
    use super::{format_size, is_image_file};
    use std::path::Path;

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
}
