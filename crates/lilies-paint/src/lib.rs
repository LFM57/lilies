#![deny(unsafe_code)]

use lilies_layout::Rect;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaintError {
    #[error("Paint error: {0}")]
    Generic(String),
}

/// A color representation (RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// A command in the display list.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    SolidColor {
        color: Color,
        rect: Rect,
    },
    DrawText {
        text: String,
        x: f32,
        y: f32,
        color: Color,
    },
}

/// A list of display commands to be rasterized to a 2D bitmap.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayList {
    pub commands: Vec<DisplayCommand>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, command: DisplayCommand) {
        self.commands.push(command);
    }
}
