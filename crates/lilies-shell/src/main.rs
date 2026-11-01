#![deny(unsafe_code)]

use lilies_css::{compute_styles, StyleSheet, StyleTree};
use lilies_dom::{Document, NodeId};
use lilies_html::parse_html;
use lilies_layout::{build_layout_tree, layout, Dimensions, Rect};
use lilies_paint::{build_display_list, rasterize, Color, Pixmap};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};

pub const DEFAULT_HTML: &str = r#"<html>
  <body style="background-color: #1e1e2e; color: #cdd6f4;">
    <div style="background-color: #313244; width: 600px; margin: 40px auto; padding: 24px;">
      <h1 style="color: #89b4fa; font-size: 32px; margin-bottom: 16px;">lilies browser</h1>
      <p style="color: #a6adc8; font-size: 16px; margin-bottom: 12px;">
        Welcome to lilies, an independent web browser rendering engine built from first principles in Rust.
      </p>
      <div style="background-color: #45475a; padding: 12px; margin-top: 16px;">
        <p style="color: #a6e3a1; font-size: 14px;">
          This text is automatically wrapped and rendered with pure software alpha blending.
        </p>
      </div>
    </div>
  </body>
</html>"#;

/// Re-runs the layout engine and rasterizer for the document given the viewport dimensions.
pub fn render_to_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    let mut root_box = build_layout_tree(html_id, doc.arena(), styles)?;
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, width as f32, height as f32));
    layout(&mut root_box, viewport);

    let display_list = build_display_list(&root_box);

    let bg_color = doc
        .get_children(html_id)
        .into_iter()
        .find_map(|child| {
            if doc.node(child)?.as_element()?.tag_name() == "body" {
                styles.get(child).map(|s| s.background_color)
            } else {
                None
            }
        })
        .filter(|c| c.a > 0)
        .or_else(|| {
            styles
                .get(html_id)
                .map(|s| s.background_color)
                .filter(|c| c.a > 0)
        })
        .unwrap_or(Color::WHITE);

    let mut pixmap = Pixmap::new(width, height, bg_color);
    rasterize(&display_list, &mut pixmap);
    Some(pixmap)
}

/// Copies RGBA pixels from `Pixmap` into a `softbuffer` `u32` slice in `0x00RRGGBB` format.
pub fn copy_pixmap_to_softbuffer(pixmap: &Pixmap, buffer: &mut [u32]) {
    for (chunk, dst) in pixmap
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(buffer.iter_mut())
    {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        *dst = (r << 16) | (g << 8) | b;
    }
}

/// Executes dynamic layout, rasterization, and presentation onto the window surface.
fn render_frame(
    surface: &mut Surface<Arc<Window>, Arc<Window>>,
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
        return Ok(());
    };

    surface.resize(w, h)?;

    let pixmap = render_to_pixmap(doc, html_id, styles, width, height)
        .ok_or("Failed to build layout tree from root")?;

    let mut buffer = surface.buffer_mut()?;
    copy_pixmap_to_softbuffer(&pixmap, &mut buffer);
    buffer.present()?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "=== lilies: Modern Browser Engine (Milestone 6 - Text Layout & Font Rasterization) ==="
    );

    // Step 1: Parse HTML & compute styles once (document state)
    let html_content = if let Some(path) = std::env::args().nth(1) {
        std::fs::read_to_string(&path).unwrap_or_else(|err| {
            eprintln!(
                "Could not read '{}': {}. Using default demo HTML.",
                path, err
            );
            DEFAULT_HTML.to_string()
        })
    } else {
        DEFAULT_HTML.to_string()
    };

    let doc = parse_html(&html_content);
    let stylesheet = StyleSheet::new();
    let styles = compute_styles(&doc, &stylesheet);

    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("Root <html> element required in document");

    println!(
        "Document parsed successfully (DOM nodes: {})",
        doc.arena().len()
    );

    // Step 2: Initialize EventLoop and Window titled "lilies"
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("lilies")
            .with_inner_size(LogicalSize::new(800.0, 600.0))
            .build(&event_loop)?,
    );

    // Step 3: Initialize softbuffer Context and Surface
    let context = Context::new(window.clone())?;
    let mut surface = Surface::new(&context, window.clone())?;

    // Initial render with current window size
    let size = window.inner_size();
    if let Err(e) = render_frame(
        &mut surface,
        &doc,
        html_id,
        &styles,
        size.width,
        size.height,
    ) {
        eprintln!("Initial render warning: {e}");
    }

    // Step 4: Run event loop
    event_loop.run(move |event, elwt| match event {
        Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
            WindowEvent::Resized(physical_size) => {
                if let Err(e) = render_frame(
                    &mut surface,
                    &doc,
                    html_id,
                    &styles,
                    physical_size.width,
                    physical_size.height,
                ) {
                    eprintln!("Render error on resize: {e}");
                }
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                if let Err(e) = render_frame(
                    &mut surface,
                    &doc,
                    html_id,
                    &styles,
                    size.width,
                    size.height,
                ) {
                    eprintln!("Render error on redraw: {e}");
                }
            }
            WindowEvent::CloseRequested => {
                elwt.exit();
            }
            _ => {}
        },
        _ => {}
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_pixmap_to_softbuffer() {
        let mut pixmap = Pixmap::new(2, 2, Color::WHITE);
        pixmap.set_pixel(0, 0, Color::rgba(0x2b, 0x3a, 0x4a, 0xff));
        pixmap.set_pixel(1, 0, Color::rgba(0xff, 0x57, 0x22, 0xff));
        pixmap.set_pixel(0, 1, Color::rgba(0x00, 0x00, 0x00, 0xff));
        pixmap.set_pixel(1, 1, Color::rgba(0xff, 0xff, 0xff, 0xff));

        let mut buffer = vec![0u32; 4];
        copy_pixmap_to_softbuffer(&pixmap, &mut buffer);

        assert_eq!(buffer[0], 0x002B3A4A);
        assert_eq!(buffer[1], 0x00FF5722);
        assert_eq!(buffer[2], 0x00000000);
        assert_eq!(buffer[3], 0x00FFFFFF);
    }

    #[test]
    fn test_card_centering_across_different_viewport_sizes() {
        const CARD_HTML: &str = r#"<html>
  <body style="background-color: #ffffff;">
    <div class="card" style="background-color: #2b3a4a; width: 400px; height: 300px; margin: 50px auto;">
      <div class="inner" style="background-color: #ff5722; width: 200px; height: 100px; margin: 20px;"></div>
    </div>
  </body>
</html>"#;
        let doc = parse_html(CARD_HTML);
        let stylesheet = StyleSheet::new();
        let styles = compute_styles(&doc, &stylesheet);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .expect("html element");

        // 1. Viewport 800x600:
        // Card width: 400. Remaining width: 800 - 400 = 400.
        // Auto margins split equally: margin-left = 200, margin-right = 200.
        // Card spans x in [200, 600).
        let pixmap_800 = render_to_pixmap(&doc, html_id, &styles, 800, 600).expect("pixmap");
        assert_eq!(pixmap_800.width, 800);
        assert_eq!(pixmap_800.height, 600);

        let card_color = Color::rgba(0x2b, 0x3a, 0x4a, 0xff);
        let bg_color = Color::rgba(0xff, 0xff, 0xff, 0xff);

        // At y = 60 (inside card vertically, margin-top is 50):
        // Just outside left edge
        assert_eq!(pixmap_800.get_pixel(199, 60), Some(bg_color));
        // Inside card
        assert_eq!(pixmap_800.get_pixel(200, 60), Some(card_color));
        assert_eq!(pixmap_800.get_pixel(599, 60), Some(card_color));
        // Just outside right edge
        assert_eq!(pixmap_800.get_pixel(600, 60), Some(bg_color));

        // 2. Viewport resized to 1000x600:
        // Card width: 400. Remaining width: 1000 - 400 = 600.
        // margin-left = 300, margin-right = 300.
        // Card spans x in [300, 700).
        let pixmap_1000 = render_to_pixmap(&doc, html_id, &styles, 1000, 600).expect("pixmap");
        assert_eq!(pixmap_1000.width, 1000);
        assert_eq!(pixmap_1000.height, 600);

        assert_eq!(pixmap_1000.get_pixel(299, 60), Some(bg_color));
        assert_eq!(pixmap_1000.get_pixel(300, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(699, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(700, 60), Some(bg_color));

        // 3. Viewport resized to 600x600:
        // Card width: 400. Remaining width: 600 - 400 = 200.
        // margin-left = 100, margin-right = 100.
        // Card spans x in [100, 500).
        let pixmap_600 = render_to_pixmap(&doc, html_id, &styles, 600, 600).expect("pixmap");
        assert_eq!(pixmap_600.width, 600);
        assert_eq!(pixmap_600.height, 600);

        assert_eq!(pixmap_600.get_pixel(99, 60), Some(bg_color));
        assert_eq!(pixmap_600.get_pixel(100, 60), Some(card_color));
        assert_eq!(pixmap_600.get_pixel(499, 60), Some(card_color));
        assert_eq!(pixmap_600.get_pixel(500, 60), Some(bg_color));
    }

    #[test]
    fn test_milestone6_rich_document_render() {
        let doc = parse_html(DEFAULT_HTML);
        let stylesheet = StyleSheet::new();
        let styles = compute_styles(&doc, &stylesheet);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .expect("html element");

        let pixmap = render_to_pixmap(&doc, html_id, &styles, 800, 600).expect("pixmap");
        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);

        let body_bg = Color::rgba(0x1e, 0x1e, 0x2e, 0xff);
        let container_bg = Color::rgba(0x31, 0x32, 0x44, 0xff);

        // Outside container (e.g., at x = 50, y = 50): body background
        assert_eq!(pixmap.get_pixel(50, 50), Some(body_bg));

        // Inside container (e.g. at container padding area: x = 80, y = 45): container background
        assert_eq!(pixmap.get_pixel(80, 45), Some(container_bg));

        // Container is 600px width + 24px left padding + 24px right padding = 648px.
        // Centered in 800px viewport: margin-left is (800 - 648) / 2 = 76px.
        // Padding box spans [76, 724).
        assert_eq!(pixmap.get_pixel(75, 50), Some(body_bg));
        assert_eq!(pixmap.get_pixel(76, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(723, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(724, 50), Some(body_bg));

        // There should be rasterized text pixels (which differ from both body_bg and container_bg)
        let text_pixels_count = pixmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|chunk| {
                let color = Color::rgba(chunk[0], chunk[1], chunk[2], chunk[3]);
                color != body_bg && color != container_bg
            })
            .count();

        assert!(
            text_pixels_count > 100,
            "Expected rasterized text pixels, found {}",
            text_pixels_count
        );

        let _ = pixmap.save_ppm("../../output.ppm");
    }

    #[test]
    fn test_milestone6_dynamic_text_reflow_on_resize() {
        let doc = parse_html(DEFAULT_HTML);
        let stylesheet = StyleSheet::new();
        let styles = compute_styles(&doc, &stylesheet);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .expect("html element");

        // Layout at 800x600
        let mut root_800 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_800,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );

        // Layout at 1200x800
        let mut root_1200 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_1200,
            Dimensions::new(Rect::new(0.0, 0.0, 1200.0, 800.0)),
        );

        // The card is centered in both
        let body_800 = &root_800.children[0];
        let card_800 = &body_800.children[0];
        assert_eq!(card_800.dimensions.margin.left, 76.0);
        assert_eq!(card_800.dimensions.content.width, 600.0);
        assert_eq!(card_800.dimensions.content.x, 100.0);

        let body_1200 = &root_1200.children[0];
        let card_1200 = &body_1200.children[0];
        assert_eq!(card_1200.dimensions.margin.left, 276.0);
        assert_eq!(card_1200.dimensions.content.width, 600.0);
        assert_eq!(card_1200.dimensions.content.x, 300.0);

        // Verify reflow when container has auto width on viewport resize:
        let auto_html = r#"<html><body><p>One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen</p></body></html>"#;
        let auto_doc = parse_html(auto_html);
        let auto_styles = compute_styles(&auto_doc, &stylesheet);
        let auto_html_id = auto_doc
            .get_children(auto_doc.root())
            .into_iter()
            .find(|&id| {
                auto_doc
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        // Wide viewport (800px): text fits on fewer lines
        let mut wide_box = build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut wide_box,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );
        let wide_p = &wide_box.children[0].children[0];
        let wide_text = &wide_p.children[0];
        let wide_line_count = wide_text.lines.len();

        // Narrow viewport (150px): text wraps into significantly more lines!
        let mut narrow_box =
            build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut narrow_box,
            Dimensions::new(Rect::new(0.0, 0.0, 150.0, 600.0)),
        );
        let narrow_p = &narrow_box.children[0].children[0];
        let narrow_text = &narrow_p.children[0];
        let narrow_line_count = narrow_text.lines.len();

        assert!(
            narrow_line_count > wide_line_count,
            "Narrow line count ({}) should be greater than wide line count ({})",
            narrow_line_count,
            wide_line_count
        );
        assert!(
            narrow_p.dimensions.content.height > wide_p.dimensions.content.height,
            "Narrow height ({}) should be greater than wide height ({})",
            narrow_p.dimensions.content.height,
            wide_p.dimensions.content.height
        );
    }
}
