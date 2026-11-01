#![deny(unsafe_code)]

use lilies_css::Color;
use lilies_layout::{EdgeSizes, Rect};

/// A single rendering primitive in the display list.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    /// A solid-color filled rectangle.
    SolidColor { rect: Rect, color: Color },
    /// A box border with specified edge widths and color.
    Border {
        rect: Rect,
        widths: EdgeSizes,
        color: Color,
    },
    /// A single line of text with position and styling.
    Text {
        text: String,
        rect: Rect,
        color: Color,
        font_size: f32,
    },
}

impl DisplayItem {
    /// Translates the display item by (dx, dy).
    pub fn offset(&mut self, dx: f32, dy: f32) {
        match self {
            DisplayItem::SolidColor { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
            DisplayItem::Border { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
            DisplayItem::Text { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
        }
    }

    /// Returns the bounding rectangle of this display item.
    pub fn rect(&self) -> Rect {
        match self {
            DisplayItem::SolidColor { rect, .. } => *rect,
            DisplayItem::Border { rect, .. } => *rect,
            DisplayItem::Text { rect, .. } => *rect,
        }
    }
}

/// An ordered list of display items to be rasterized to a 2D framebuffer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
}

impl DisplayList {
    /// Creates a new empty display list.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Translates all display items in the list by (dx, dy).
    pub fn offset(&mut self, dx: f32, dy: f32) {
        for item in &mut self.items {
            item.offset(dx, dy);
        }
    }

    /// Computes the maximum bottom extent (y + height) across all items.
    pub fn total_height(&self) -> f32 {
        self.items
            .iter()
            .map(|item| {
                let r = item.rect();
                r.y + r.height
            })
            .fold(0.0f32, f32::max)
    }

    /// Appends a display item to the list.
    pub fn push(&mut self, item: DisplayItem) {
        self.items.push(item);
    }

    /// Returns an iterator over the display items.
    pub fn iter(&self) -> std::slice::Iter<'_, DisplayItem> {
        self.items.iter()
    }

    /// Clears all display items from the list.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Returns the number of items in the display list.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the display list contains no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns a slice of the items.
    pub fn items(&self) -> &[DisplayItem] {
        &self.items
    }

    /// Consumes the display list and returns the underlying `Vec<DisplayItem>`.
    pub fn into_inner(self) -> Vec<DisplayItem> {
        self.items
    }
}

impl IntoIterator for DisplayList {
    type Item = DisplayItem;
    type IntoIter = std::vec::IntoIter<DisplayItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a DisplayList {
    type Item = &'a DisplayItem;
    type IntoIter = std::slice::Iter<'a, DisplayItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}
