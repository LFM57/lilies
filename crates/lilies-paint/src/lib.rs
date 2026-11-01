#![deny(unsafe_code)]

pub mod builder;
pub mod display_item;
pub mod error;
pub mod rasterizer;

pub use builder::build_display_list;
pub use display_item::{DisplayItem, DisplayList};
pub use error::PaintError;
pub use lilies_css::Color;
pub use lilies_net::DecodedImage;
pub use rasterizer::{draw_image, rasterize, Pixmap};

#[cfg(test)]
mod tests;
