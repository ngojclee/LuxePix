use std::fs;
use std::path::{Path, PathBuf};

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff", "tif", "ico", "avif",
];

pub struct FolderEntry {
    pub path: PathBuf,
    pub name: String,
    pub first_image: Option<PathBuf>,
    pub image_count: usize,
    pub total_files: usize,
}

/// A file entry — either an image or a generic file.
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_image: bool,
    pub extension: String,
    pub size_bytes: u64,
}

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan a directory and return info about each sub-folder (first image + count).
pub fn scan_directory(path: &Path) -> Vec<FolderEntry> {
    let mut folders = Vec::new();

    let Ok(entries) = fs::read_dir(path) else {
        return folders;
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().ok().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();

    dirs.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    for dir in dirs {
        let all = list_all_files(&dir);
        let first_img = all.iter().find(|f| f.is_image).map(|f| f.path.clone());
        let img_count = all.iter().filter(|f| f.is_image).count();
        let total = all.len();
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        folders.push(FolderEntry {
            path: dir,
            name,
            first_image: first_img,
            image_count: img_count,
            total_files: total,
        });
    }

    folders
}

/// List ALL files in a directory (non-recursive), sorted naturally.
/// Images come first, then other files.
pub fn list_all_files(path: &Path) -> Vec<FileEntry> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
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
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

            files.push(FileEntry {
                is_image: is_image_file(&p),
                path: p,
                name,
                extension: ext,
                size_bytes: size,
            });
        }
    }

    // Sort: images first, then by name naturally
    files.sort_by(|a, b| {
        b.is_image
            .cmp(&a.is_image)
            .then_with(|| natord::compare(&a.name, &b.name))
    });

    files
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
