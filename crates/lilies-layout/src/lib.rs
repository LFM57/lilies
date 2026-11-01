#![deny(unsafe_code)]

pub mod box_model;
pub mod error;
pub mod font;
pub mod hit_test;
pub mod layout_box;
pub mod tree_builder;

pub use box_model::{Dimensions, EdgeSizes, Rect};
pub use error::LayoutError;
pub use font::{default_font, wrap_text, TextLine};
pub use hit_test::{find_parent_link, hit_test};
pub use layout_box::{BoxType, LayoutBox};
pub use lilies_css::TextAlign;
pub use tree_builder::build_layout_tree;

/// Calculates the layout of a root box within the given initial viewport dimensions.
pub fn layout(root_box: &mut LayoutBox, viewport: Dimensions) {
    let mut initial_cb = viewport;
    initial_cb.content.height = 0.0;
    root_box.layout(initial_cb);
}

#[cfg(test)]
mod tests;
