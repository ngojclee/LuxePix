use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;

pub struct ThumbResult {
    pub path: PathBuf,
    pub image: Option<egui::ColorImage>,
    pub original_size: Option<[u32; 2]>,
}

struct CacheEntry {
    texture: egui::TextureHandle,
    original_size: Option<[u32; 2]>,
}

pub struct ThumbManager {
    cache: HashMap<PathBuf, CacheEntry>,
    pending: HashSet<PathBuf>,
    result_tx: mpsc::Sender<ThumbResult>,
    result_rx: mpsc::Receiver<ThumbResult>,
    pool: rayon::ThreadPool,
    max_cache: usize,
}

impl ThumbManager {
    pub fn new(max_cache: usize, threads: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();

        Self {
            cache: HashMap::new(),
            pending: HashSet::new(),
            result_tx: tx,
            result_rx: rx,
            pool,
            max_cache,
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
                        // Evict if full
                        if self.cache.len() >= self.max_cache {
                            let oldest = self.cache.keys().next().cloned();
                            if let Some(key) = oldest {
                                self.cache.remove(&key);
                            }
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

    pub fn get(&self, path: &Path) -> Option<&egui::TextureHandle> {
        self.cache.get(path).map(|e| &e.texture)
    }

    pub fn get_original_size(&self, path: &Path) -> Option<[u32; 2]> {
        self.cache.get(path).and_then(|e| e.original_size)
    }

    pub fn is_pending(&self, path: &Path) -> bool {
        self.pending.contains(path)
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.pending.clear();
        // drain the channel
        while self.result_rx.try_recv().is_ok() {}
    }

    pub fn has(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }
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
