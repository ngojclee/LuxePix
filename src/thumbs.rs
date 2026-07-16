use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;

const RGBA_BYTES_PER_PIXEL: usize = 4;

pub struct ThumbResult {
    pub path: PathBuf,
    pub image: Option<egui::ColorImage>,
    pub original_size: Option<[u32; 2]>,
}

struct CacheEntry {
    texture: egui::TextureHandle,
    original_size: Option<[u32; 2]>,
}

#[derive(Debug)]
struct CacheRecord {
    bytes: usize,
    last_used: u64,
}

#[derive(Debug)]
struct CacheAdmission {
    admitted: bool,
    evicted: Vec<PathBuf>,
}

#[derive(Debug)]
struct ByteBoundedLru {
    entries: HashMap<PathBuf, CacheRecord>,
    max_bytes: usize,
    cached_bytes: usize,
    clock: u64,
}

impl ByteBoundedLru {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes,
            cached_bytes: 0,
            clock: 0,
        }
    }

    fn insert(&mut self, path: PathBuf, bytes: usize) -> CacheAdmission {
        self.remove(&path);
        if bytes > self.max_bytes {
            return CacheAdmission {
                admitted: false,
                evicted: Vec::new(),
            };
        }

        let mut evicted = Vec::new();
        while self.cached_bytes.saturating_add(bytes) > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by(|(left_path, left), (right_path, right)| {
                    left.last_used
                        .cmp(&right.last_used)
                        .then_with(|| left_path.cmp(right_path))
                })
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.remove(&oldest);
            evicted.push(oldest);
        }

        self.clock = self.clock.saturating_add(1);
        self.cached_bytes = self.cached_bytes.saturating_add(bytes);
        self.entries.insert(
            path,
            CacheRecord {
                bytes,
                last_used: self.clock,
            },
        );
        CacheAdmission {
            admitted: true,
            evicted,
        }
    }

    fn touch(&mut self, path: &Path) {
        let Some(entry) = self.entries.get_mut(path) else {
            return;
        };
        self.clock = self.clock.saturating_add(1);
        entry.last_used = self.clock;
    }

    fn set_max_bytes(&mut self, max_bytes: usize) -> Vec<PathBuf> {
        self.max_bytes = max_bytes;
        let mut evicted = Vec::new();
        while self.cached_bytes > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by(|(left_path, left), (right_path, right)| {
                    left.last_used
                        .cmp(&right.last_used)
                        .then_with(|| left_path.cmp(right_path))
                })
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            self.remove(&oldest);
            evicted.push(oldest);
        }
        evicted
    }

    fn remove(&mut self, path: &Path) {
        if let Some(entry) = self.entries.remove(path) {
            self.cached_bytes = self.cached_bytes.saturating_sub(entry.bytes);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.cached_bytes = 0;
    }
}

pub struct ThumbManager {
    cache: HashMap<PathBuf, CacheEntry>,
    cache_policy: ByteBoundedLru,
    pending: HashSet<PathBuf>,
    result_tx: mpsc::SyncSender<ThumbResult>,
    result_rx: mpsc::Receiver<ThumbResult>,
    pool: rayon::ThreadPool,
}

impl ThumbManager {
    pub fn new(max_cache_bytes: usize, threads: usize) -> Self {
        let threads = threads.max(1);
        let (tx, rx) = mpsc::sync_channel(threads.saturating_mul(2));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();

        Self {
            cache: HashMap::new(),
            cache_policy: ByteBoundedLru::new(max_cache_bytes),
            pending: HashSet::new(),
            result_tx: tx,
            result_rx: rx,
            pool,
        }
    }

    /// Request a thumbnail (resized to max_size). Non-blocking.
    pub fn request_thumbnail(&mut self, path: &Path, max_size: u32) {
        if self.cache.contains_key(path) || self.pending.contains(path) {
            return;
        }

        self.pending.insert(path.to_path_buf());
        let path_owned = path.to_path_buf();
        let tx = self.result_tx.clone();

        self.pool.spawn(move || {
            let result = load_and_resize(&path_owned, max_size);
            let _ = tx.send(result);
        });
    }

    /// Request a full-resolution image. Non-blocking.
    pub fn request_full(&mut self, path: &Path) {
        if self.cache.contains_key(path) || self.pending.contains(path) {
            return;
        }

        self.pending.insert(path.to_path_buf());
        let path_owned = path.to_path_buf();
        let tx = self.result_tx.clone();

        self.pool.spawn(move || {
            let result = load_full(&path_owned);
            let _ = tx.send(result);
        });
    }

    /// Process pending results (call once per frame). Returns true if any new textures were loaded.
    pub fn process_results(&mut self, ctx: &egui::Context) -> bool {
        let mut loaded = false;
        for _ in 0..20 {
            match self.result_rx.try_recv() {
                Ok(result) => {
                    self.pending.remove(&result.path);
                    if let Some(color_image) = result.image {
                        let bytes = estimated_rgba_bytes(&color_image);
                        let admission = self.cache_policy.insert(result.path.clone(), bytes);
                        for path in admission.evicted {
                            self.cache.remove(&path);
                        }
                        if !admission.admitted {
                            continue;
                        }

                        let texture = ctx.load_texture(
                            result.path.to_string_lossy(),
                            color_image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.cache.insert(
                            result.path,
                            CacheEntry {
                                texture,
                                original_size: result.original_size,
                            },
                        );
                        loaded = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        loaded
    }

    pub fn get(&mut self, path: &Path) -> Option<&egui::TextureHandle> {
        self.cache_policy.touch(path);
        self.cache.get(path).map(|e| &e.texture)
    }

    pub fn get_original_size(&mut self, path: &Path) -> Option<[u32; 2]> {
        self.cache_policy.touch(path);
        self.cache.get(path).and_then(|e| e.original_size)
    }

    pub fn set_byte_budget(&mut self, max_cache_bytes: usize) {
        for path in self.cache_policy.set_max_bytes(max_cache_bytes) {
            self.cache.remove(&path);
        }
    }

    pub const fn byte_budget(&self) -> usize {
        self.cache_policy.max_bytes
    }

    pub const fn cached_bytes(&self) -> usize {
        self.cache_policy.cached_bytes
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.cache_policy.clear();
        self.pending.clear();
        // drain the channel
        while self.result_rx.try_recv().is_ok() {}
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }
}

fn estimated_rgba_bytes(image: &egui::ColorImage) -> usize {
    image
        .size
        .iter()
        .copied()
        .try_fold(RGBA_BYTES_PER_PIXEL, usize::checked_mul)
        .unwrap_or(usize::MAX)
}

fn load_and_resize(path: &Path, max_size: u32) -> ThumbResult {
    match image::open(path) {
        Ok(img) => {
            let (w, h) = (img.width(), img.height());
            let thumb = img.thumbnail(max_size, max_size);
            let rgba = thumb.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.into_raw();

            ThumbResult {
                path: path.to_path_buf(),
                image: Some(egui::ColorImage::from_rgba_unmultiplied(size, &pixels)),
                original_size: Some([w, h]),
            }
        }
        Err(_) => ThumbResult {
            path: path.to_path_buf(),
            image: None,
            original_size: None,
        },
    }
}

fn load_full(path: &Path) -> ThumbResult {
    match image::open(path) {
        Ok(img) => {
            let (w, h) = (img.width(), img.height());
            // Cap at 4096 for display performance
            let display = if w > 4096 || h > 4096 {
                img.thumbnail(4096, 4096)
            } else {
                img
            };
            let rgba = display.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.into_raw();

            ThumbResult {
                path: path.to_path_buf(),
                image: Some(egui::ColorImage::from_rgba_unmultiplied(size, &pixels)),
                original_size: Some([w, h]),
            }
        }
        Err(_) => ThumbResult {
            path: path.to_path_buf(),
            image: None,
            original_size: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::ByteBoundedLru;
    use std::path::{Path, PathBuf};

    #[test]
    fn byte_bounded_lru_evicts_the_least_recently_used_entry() {
        let mut policy = ByteBoundedLru::new(80);
        assert!(policy.insert(PathBuf::from("a"), 40).admitted);
        assert!(policy.insert(PathBuf::from("b"), 40).admitted);
        policy.touch(Path::new("a"));

        let admission = policy.insert(PathBuf::from("c"), 40);

        assert!(admission.admitted);
        assert_eq!(admission.evicted, vec![PathBuf::from("b")]);
        assert!(policy.entries.contains_key(Path::new("a")));
        assert!(policy.entries.contains_key(Path::new("c")));
        assert_eq!(policy.cached_bytes, 80);
    }

    #[test]
    fn byte_bounded_lru_rejects_an_entry_larger_than_the_budget() {
        let mut policy = ByteBoundedLru::new(64);

        let admission = policy.insert(PathBuf::from("oversized"), 65);

        assert!(!admission.admitted);
        assert!(admission.evicted.is_empty());
        assert!(policy.entries.is_empty());
        assert_eq!(policy.cached_bytes, 0);
    }

    #[test]
    fn lowering_the_budget_evicts_until_usage_is_within_the_limit() {
        let mut policy = ByteBoundedLru::new(120);
        policy.insert(PathBuf::from("a"), 40);
        policy.insert(PathBuf::from("b"), 40);
        policy.insert(PathBuf::from("c"), 40);
        policy.touch(Path::new("a"));

        let evicted = policy.set_max_bytes(80);

        assert_eq!(evicted, vec![PathBuf::from("b")]);
        assert!(policy.cached_bytes <= policy.max_bytes);
    }
}
