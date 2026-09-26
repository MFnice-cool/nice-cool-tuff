//! LRU GPU texture cache for YouTube thumbnails.

use crate::gl;
use lru::LruCache;
use parking_lot::Mutex;
use std::io::Read;
use std::num::NonZeroUsize;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const MAX_TEXTURES: usize = 30;

enum Job {
    Fetch { key: String, url: String },
}

struct Decoded {
    key: String,
    width: i32,
    height: i32,
    rgba: Vec<u8>,
}

pub struct TextureCache {
    cache: LruCache<String, u32>,
    inflight: std::collections::HashSet<String>,
    tx: Sender<Job>,
    rx: Receiver<Decoded>,
}

impl TextureCache {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (dec_tx, dec_rx) = mpsc::channel::<Decoded>();
        thread::Builder::new()
            .name("overlay-thumbs".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    match job {
                        Job::Fetch { key, url } => {
                            if let Some(decoded) = download_decode(&url) {
                                let _ = dec_tx.send(Decoded {
                                    key,
                                    width: decoded.0,
                                    height: decoded.1,
                                    rgba: decoded.2,
                                });
                            }
                        }
                    }
                }
            })
            .ok();
        Self {
            cache: LruCache::new(NonZeroUsize::new(MAX_TEXTURES).unwrap()),
            inflight: std::collections::HashSet::new(),
            tx: job_tx,
            rx: dec_rx,
        }
    }

    /// Pump completed downloads onto the GL thread (call from swap hook).
    pub fn pump_uploads(&mut self) {
        while let Ok(decoded) = self.rx.try_recv() {
            self.inflight.remove(&decoded.key);
            let tex = gl::create_rgba_texture(decoded.width, decoded.height, &decoded.rgba);
            if let Some((_, old)) = self.cache.push(decoded.key, tex) {
                gl::delete_texture(old);
            }
        }
    }

    pub fn get_or_request(&mut self, key: &str, url: &str) -> Option<u32> {
        if let Some(tex) = self.cache.get(key) {
            return Some(*tex);
        }
        if !url.is_empty() && !self.inflight.contains(key) {
            self.inflight.insert(key.to_string());
            let _ = self.tx.send(Job::Fetch {
                key: key.to_string(),
                url: url.to_string(),
            });
        }
        None
    }
}

fn download_decode(url: &str) -> Option<(i32, i32, Vec<u8>)> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let w = img.width() as i32;
    let h = img.height() as i32;
    Some((w, h, img.into_raw()))
}

/// Shared singleton for the UI.
pub static TEXTURES: Mutex<Option<TextureCache>> = Mutex::new(None);

pub fn with_textures<R>(f: impl FnOnce(&mut TextureCache) -> R) -> Option<R> {
    let mut g = TEXTURES.lock();
    if g.is_none() {
        *g = Some(TextureCache::new());
    }
    g.as_mut().map(f)
}
