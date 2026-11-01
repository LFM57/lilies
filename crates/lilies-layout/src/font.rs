#![deny(unsafe_code)]

use crate::box_model::Rect;
use fontdue::{Font, FontSettings};
use std::sync::OnceLock;

/// The embedded fallback TrueType font (Roboto-Regular).
pub const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");

static DEFAULT_FONT: OnceLock<Font> = OnceLock::new();

/// Returns a reference to the shared default font.
///
/// Attempts to load from `assets/fonts/Roboto-Regular.ttf` if available on disk,
/// and falls back seamlessly to the embedded font data.
pub fn default_font() -> &'static Font {
    DEFAULT_FONT.get_or_init(|| {
        if let Ok(bytes) = std::fs::read("assets/fonts/Roboto-Regular.ttf") {
            if let Ok(font) = Font::from_bytes(bytes.as_slice(), FontSettings::default()) {
                return font;
            }
        }
        Font::from_bytes(DEFAULT_FONT_BYTES, FontSettings::default())
            .expect("Failed to parse embedded default font")
    })
}

/// Measures the horizontal advance width of a single character in pixels.
pub fn measure_char(c: char, font_size: f32) -> f32 {
    default_font().metrics(c, font_size).advance_width
}

/// Measures the total horizontal advance width of a text string in pixels.
pub fn measure_text(text: &str, font_size: f32) -> f32 {
    let font = default_font();
    text.chars()
        .map(|c| font.metrics(c, font_size).advance_width)
        .sum()
}

/// Returns the standard line height for a given font size.
pub fn line_height(font_size: f32) -> f32 {
    font_size * 1.2
}

/// Returns the font ascent in pixels for a given font size.
pub fn font_ascent(font_size: f32) -> f32 {
    default_font()
        .horizontal_line_metrics(font_size)
        .map_or(font_size * 0.8, |lm| lm.ascent)
}

/// A single wrapped line of text with its bounding rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub text: String,
    pub rect: Rect,
}

/// Splits text into words and wraps them across lines such that no line exceeds `max_width`.
///
/// If `max_width <= 0.0`, all words are placed on separate lines (or single line if empty).
pub fn wrap_text(
    text: &str,
    font_size: f32,
    max_width: f32,
    start_x: f32,
    start_y: f32,
) -> Vec<TextLine> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }

    let font = default_font();
    let space_width = font.metrics(' ', font_size).advance_width;
    let line_h = line_height(font_size);

    let mut lines = Vec::new();
    let mut current_words: Vec<&str> = Vec::new();
    let mut current_width = 0.0f32;

    for word in words {
        let word_width: f32 = word
            .chars()
            .map(|c| font.metrics(c, font_size).advance_width)
            .sum();

        if current_words.is_empty() {
            current_words.push(word);
            current_width = word_width;
        } else {
            let candidate_width = current_width + space_width + word_width;
            if candidate_width <= max_width || max_width <= 0.0 {
                current_words.push(word);
                current_width = candidate_width;
            } else {
                let line_y = start_y + (lines.len() as f32) * line_h;
                lines.push(TextLine {
                    text: current_words.join(" "),
                    rect: Rect::new(start_x, line_y, current_width, line_h),
                });
                current_words = vec![word];
                current_width = word_width;
            }
        }
    }

    if !current_words.is_empty() {
        let line_y = start_y + (lines.len() as f32) * line_h;
        lines.push(TextLine {
            text: current_words.join(" "),
            rect: Rect::new(start_x, line_y, current_width, line_h),
        });
    }

    lines
}
