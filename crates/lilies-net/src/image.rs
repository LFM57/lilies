#![deny(unsafe_code)]

use crate::error::NetError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Errors that can occur during image decoding or caching.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("Image decode error: {0}")]
    Decode(#[from] image::ImageError),

    #[error("Empty image data")]
    EmptyData,

    #[error("Network error fetching image: {0}")]
    Net(#[from] NetError),
}

/// A decoded RGBA image representation (RGBA8888, 4 bytes per pixel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA8888
}

impl DecodedImage {
    /// Creates a new decoded image with the specified dimensions and RGBA pixel buffer.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// Decodes binary image data into a `DecodedImage` (RGBA8888).
///
/// Supports PNG, JPEG, WebP, and GIF formats.
pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    if bytes.is_empty() {
        return Err(ImageError::EmptyData);
    }

    let img = image::load_from_memory(bytes)?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(DecodedImage {
        width,
        height,
        pixels: rgba.into_raw(),
    })
}

/// A thread-safe cache of decoded images keyed by URL.
#[derive(Debug, Clone, Default)]
pub struct ImageCache {
    entries: Arc<RwLock<HashMap<String, Arc<DecodedImage>>>>,
}

impl ImageCache {
    /// Creates an empty image cache.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieves a cached image by URL.
    pub fn get(&self, url: &str) -> Option<Arc<DecodedImage>> {
        self.entries.read().ok()?.get(url).cloned()
    }

    /// Stores a decoded image into the cache.
    pub fn insert(&self, url: impl Into<String>, image: Arc<DecodedImage>) {
        if let Ok(mut map) = self.entries.write() {
            map.insert(url.into(), image);
        }
    }

    /// Removes a cached image entry by key.
    pub fn remove(&self, url: &str) -> Option<Arc<DecodedImage>> {
        self.entries.write().ok()?.remove(url)
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        if let Ok(mut map) = self.entries.write() {
            map.clear();
        }
    }

    /// Checks if the cache contains an image for the given URL.
    pub fn contains(&self, url: &str) -> bool {
        self.entries
            .read()
            .ok()
            .is_some_and(|m| m.contains_key(url))
    }

    /// Returns the number of cached images.
    pub fn len(&self) -> usize {
        self.entries.read().ok().map_or(0, |m| m.len())
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a snapshot clone of the current cached entries map.
    pub fn snapshot(&self) -> HashMap<String, Arc<DecodedImage>> {
        self.entries
            .read()
            .ok()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Fetches binary bytes from the URL, decodes them, and caches the result.
    /// If already cached, returns the cached image without re-fetching.
    pub fn fetch_and_cache(&self, url: &str) -> Result<Arc<DecodedImage>, ImageError> {
        if let Some(cached) = self.get(url) {
            return Ok(cached);
        }

        let bytes = crate::fetch::fetch_bytes(url)?;
        let decoded = decode_image(&bytes)?;
        let arc = Arc::new(decoded);
        self.insert(url, Arc::clone(&arc));
        Ok(arc)
    }
}
