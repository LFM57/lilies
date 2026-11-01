#![deny(unsafe_code)]

use crate::display_item::{DisplayItem, DisplayList};
use lilies_css::Color;
use lilies_layout::{EdgeSizes, Rect};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// An in-memory 2D RGBA pixel buffer (RGBA8888, 4 bytes per pixel).
#[derive(Debug, Clone, PartialEq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Pixmap {
    /// Creates a new pixmap initialized with the given background color.
    pub fn new(width: u32, height: u32, background: Color) -> Self {
        let total_pixels = (width as usize) * (height as usize);
        let mut pixels = Vec::with_capacity(total_pixels * 4);
        for _ in 0..total_pixels {
            pixels.push(background.r);
            pixels.push(background.g);
            pixels.push(background.b);
            pixels.push(background.a);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Retrieves the color of the pixel at coordinates `(x, y)`, or `None` if out of bounds.
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x < self.width && y < self.height {
            let idx = ((y * self.width + x) * 4) as usize;
            Some(Color::rgba(
                self.pixels[idx],
                self.pixels[idx + 1],
                self.pixels[idx + 2],
                self.pixels[idx + 3],
            ))
        } else {
            None
        }
    }

    /// Sets the pixel at coordinates `(x, y)` to the given color without blending.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            let idx = ((y * self.width + x) * 4) as usize;
            self.pixels[idx] = color.r;
            self.pixels[idx + 1] = color.g;
            self.pixels[idx + 2] = color.b;
            self.pixels[idx + 3] = color.a;
        }
    }

    /// Saves the pixmap as a binary Netpbm Portable Pixmap (PPM, format P6).
    pub fn save_ppm(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // PPM header: P6 <width> <height> 255\n
        write!(writer, "P6\n{} {}\n255\n", self.width, self.height)?;

        let mut row_buf = Vec::with_capacity((self.width * 3) as usize);
        for chunk in self.pixels.as_chunks::<4>().0 {
            row_buf.push(chunk[0]); // R
            row_buf.push(chunk[1]); // G
            row_buf.push(chunk[2]); // B
            if row_buf.len() >= 65536 {
                writer.write_all(&row_buf)?;
                row_buf.clear();
            }
        }
        if !row_buf.is_empty() {
            writer.write_all(&row_buf)?;
        }
        writer.flush()?;
        Ok(())
    }
}

/// Blends a source pixel over a destination pixel using standard Porter-Duff Source-Over compositing:
/// `dest = src + dest * (1 - src_alpha)`
fn blend_pixel(src: Color, dest: Color) -> Color {
    if src.a == 255 {
        return src;
    }
    if src.a == 0 {
        return dest;
    }

    let src_a = src.a as f32 / 255.0;
    let dst_a = dest.a as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);

    if out_a <= 0.0 {
        return Color::TRANSPARENT;
    }

    let src_weight = src_a;
    let dst_weight = dst_a * (1.0 - src_a);

    let r = ((src.r as f32 * src_weight + dest.r as f32 * dst_weight) / out_a).round() as u8;
    let g = ((src.g as f32 * src_weight + dest.g as f32 * dst_weight) / out_a).round() as u8;
    let b = ((src.b as f32 * src_weight + dest.b as f32 * dst_weight) / out_a).round() as u8;
    let a = (out_a * 255.0).round().min(255.0) as u8;

    Color::rgba(r, g, b, a)
}

/// Draws a filled rectangle onto the pixmap with clipping to the pixmap bounds and alpha blending.
fn fill_rect(pixmap: &mut Pixmap, rect: Rect, color: Color) {
    if rect.width <= 0.0 || rect.height <= 0.0 || color.a == 0 {
        return;
    }

    // Convert coordinates to integer pixel ranges with viewport clipping
    let x0 = rect.x.floor() as i64;
    let y0 = rect.y.floor() as i64;
    let x1 = (rect.x + rect.width).ceil() as i64;
    let y1 = (rect.y + rect.height).ceil() as i64;

    let min_x = x0.max(0).min(pixmap.width as i64) as u32;
    let max_x = x1.max(0).min(pixmap.width as i64) as u32;
    let min_y = y0.max(0).min(pixmap.height as i64) as u32;
    let max_y = y1.max(0).min(pixmap.height as i64) as u32;

    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let pw = pixmap.width as usize;

    if color.a == 255 {
        // Fast path for fully opaque colors
        for y in min_y..max_y {
            let row_start = (y as usize * pw + min_x as usize) * 4;
            let row_end = (y as usize * pw + max_x as usize) * 4;
            for chunk in pixmap.pixels[row_start..row_end].as_chunks_mut::<4>().0 {
                chunk[0] = color.r;
                chunk[1] = color.g;
                chunk[2] = color.b;
                chunk[3] = 255;
            }
        }
    } else {
        // Source-Over alpha blending
        for y in min_y..max_y {
            let row_start = (y as usize * pw + min_x as usize) * 4;
            let row_end = (y as usize * pw + max_x as usize) * 4;
            for chunk in pixmap.pixels[row_start..row_end].as_chunks_mut::<4>().0 {
                let dest = Color::rgba(chunk[0], chunk[1], chunk[2], chunk[3]);
                let blended = blend_pixel(color, dest);
                chunk[0] = blended.r;
                chunk[1] = blended.g;
                chunk[2] = blended.b;
                chunk[3] = blended.a;
            }
        }
    }
}

/// Draws box borders as up to 4 non-overlapping filled edge rectangles.
fn draw_border(pixmap: &mut Pixmap, rect: Rect, widths: EdgeSizes, color: Color) {
    if color.a == 0 {
        return;
    }

    // Top border
    if widths.top > 0.0 {
        fill_rect(
            pixmap,
            Rect::new(rect.x, rect.y, rect.width, widths.top),
            color,
        );
    }

    // Bottom border
    if widths.bottom > 0.0 {
        fill_rect(
            pixmap,
            Rect::new(
                rect.x,
                rect.y + rect.height - widths.bottom,
                rect.width,
                widths.bottom,
            ),
            color,
        );
    }

    // Left border (between top and bottom edges)
    if widths.left > 0.0 {
        let h = (rect.height - widths.top - widths.bottom).max(0.0);
        if h > 0.0 {
            fill_rect(
                pixmap,
                Rect::new(rect.x, rect.y + widths.top, widths.left, h),
                color,
            );
        }
    }

    // Right border (between top and bottom edges)
    if widths.right > 0.0 {
        let h = (rect.height - widths.top - widths.bottom).max(0.0);
        if h > 0.0 {
            fill_rect(
                pixmap,
                Rect::new(
                    rect.x + rect.width - widths.right,
                    rect.y + widths.top,
                    widths.right,
                    h,
                ),
                color,
            );
        }
    }
}

/// Renders a single line of text onto the pixmap using fontdue and alpha blending.
fn draw_text(pixmap: &mut Pixmap, text: &str, rect: Rect, color: Color, font_size: f32) {
    if text.is_empty() || color.a == 0 || font_size <= 0.0 {
        return;
    }

    let font = lilies_layout::font::default_font();
    let ascent = lilies_layout::font::font_ascent(font_size);
    let baseline_y = rect.y + ascent;
    let mut cursor_x = rect.x;
    let pw = pixmap.width as usize;

    for c in text.chars() {
        let (metrics, bitmap) = font.rasterize(c, font_size);
        if metrics.width > 0 && metrics.height > 0 {
            let glyph_x = cursor_x + metrics.xmin as f32;
            let glyph_top_y = baseline_y - (metrics.ymin as f32 + metrics.height as f32);

            let gx0 = glyph_x.round() as i64;
            let gy0 = glyph_top_y.round() as i64;

            for row in 0..metrics.height {
                let py = gy0 + row as i64;
                if py < 0 || py >= pixmap.height as i64 {
                    continue;
                }

                let row_offset = row * metrics.width;
                for col in 0..metrics.width {
                    let px = gx0 + col as i64;
                    if px < 0 || px >= pixmap.width as i64 {
                        continue;
                    }

                    let coverage = bitmap[row_offset + col];
                    if coverage == 0 {
                        continue;
                    }

                    let alpha = ((color.a as u32 * coverage as u32) / 255) as u8;
                    if alpha == 0 {
                        continue;
                    }

                    let glyph_pixel = Color::rgba(color.r, color.g, color.b, alpha);
                    let idx = (py as usize * pw + px as usize) * 4;
                    let dest = Color::rgba(
                        pixmap.pixels[idx],
                        pixmap.pixels[idx + 1],
                        pixmap.pixels[idx + 2],
                        pixmap.pixels[idx + 3],
                    );
                    let blended = blend_pixel(glyph_pixel, dest);
                    pixmap.pixels[idx] = blended.r;
                    pixmap.pixels[idx + 1] = blended.g;
                    pixmap.pixels[idx + 2] = blended.b;
                    pixmap.pixels[idx + 3] = blended.a;
                }
            }
        }
        cursor_x += metrics.advance_width;
    }
}

/// Rasterizes a display list onto the target pixmap.
pub fn rasterize(display_list: &DisplayList, pixmap: &mut Pixmap) {
    for item in display_list {
        match item {
            DisplayItem::SolidColor { rect, color } => {
                fill_rect(pixmap, *rect, *color);
            }
            DisplayItem::Border {
                rect,
                widths,
                color,
            } => {
                draw_border(pixmap, *rect, *widths, *color);
            }
            DisplayItem::Text {
                text,
                rect,
                color,
                font_size,
            } => {
                draw_text(pixmap, text, *rect, *color, *font_size);
            }
        }
    }
}
