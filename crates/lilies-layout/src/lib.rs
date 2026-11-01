#![deny(unsafe_code)]

use lilies_dom::NodeId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LayoutError {
    #[error("Layout error: {0}")]
    Generic(String),
}

/// Represents physical box dimensions in 2D space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A node in the layout tree.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    pub node_id: Option<NodeId>,
    pub dimensions: Rect,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(node_id: Option<NodeId>) -> Self {
        Self {
            node_id,
            dimensions: Rect::default(),
            children: Vec::new(),
        }
    }
}
