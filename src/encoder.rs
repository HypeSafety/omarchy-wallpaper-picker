use image::DynamicImage;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

/// Request to encode an image for a specific cell size
pub struct EncodeRequest {
    pub index: usize,
    pub image: DynamicImage,
    pub width: u16,
    pub height: u16,
}

/// Result of encoding an image
pub struct EncodeResult {
    pub index: usize,
    pub width: u16,
    pub height: u16,
    pub protocol: StatefulProtocol,
}

/// Cache key for encoded protocols
#[derive(Hash, Eq, PartialEq, Clone, Copy)]
pub struct CacheKey {
    pub index: usize,
    pub width: u16,
    pub height: u16,
}

/// Background image encoder that processes images in a separate thread
pub struct ImageEncoder {
    tx: Sender<EncodeRequest>,
    rx: Receiver<EncodeResult>,
    _handle: JoinHandle<()>,
    /// Cache of encoded protocols by (index, width, height)
    cache: HashMap<CacheKey, StatefulProtocol>,
    /// Track pending requests to avoid duplicates
    pending: HashMap<CacheKey, bool>,
}

impl ImageEncoder {
    pub fn new(picker: Picker) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<EncodeRequest>();
        let (res_tx, res_rx) = mpsc::channel::<EncodeResult>();

        let handle = thread::spawn(move || {
            let mut picker = picker;
            while let Ok(request) = req_rx.recv() {
                let protocol = picker.new_resize_protocol(request.image);
                let _ = res_tx.send(EncodeResult {
                    index: request.index,
                    width: request.width,
                    height: request.height,
                    protocol,
                });
            }
        });

        Self {
            tx: req_tx,
            rx: res_rx,
            _handle: handle,
            cache: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Request encoding for an image if not already cached or pending
    pub fn request_encode(
        &mut self,
        index: usize,
        image: DynamicImage,
        width: u16,
        height: u16,
    ) {
        let key = CacheKey { index, width, height };

        // Skip if already cached or pending
        if self.cache.contains_key(&key) || self.pending.contains_key(&key) {
            return;
        }

        self.pending.insert(key, true);
        let _ = self.tx.send(EncodeRequest {
            index,
            image,
            width,
            height,
        });
    }

    /// Poll for completed encodings and update cache
    pub fn poll_results(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            let key = CacheKey {
                index: result.index,
                width: result.width,
                height: result.height,
            };
            self.pending.remove(&key);
            self.cache.insert(key, result.protocol);
        }
    }

    /// Get a cached protocol if available
    pub fn get_cached(&mut self, index: usize, width: u16, height: u16) -> Option<&mut StatefulProtocol> {
        let key = CacheKey { index, width, height };
        self.cache.get_mut(&key)
    }

    /// Clear cache (e.g., when wallpapers are reloaded)
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.pending.clear();
    }

    /// Get the number of cached protocols
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}
