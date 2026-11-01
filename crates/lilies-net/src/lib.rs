#![deny(unsafe_code)]

pub mod error;
pub mod fetch;
pub mod image;

pub use error::NetError;
pub use fetch::{fetch, fetch_bytes, MAX_REDIRECTS, USER_AGENT};
pub use image::{decode_image, DecodedImage, ImageCache, ImageError};

#[cfg(test)]
mod tests;
