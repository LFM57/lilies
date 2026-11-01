#![deny(unsafe_code)]

use lilies_css::{compute_styles_for_document, StyleTree};
use lilies_dom::{Document, NodeId};
use lilies_html::parse_html;
use lilies_layout::{build_layout_tree, layout, Dimensions, EdgeSizes, Rect};
use lilies_paint::{build_display_list, rasterize, Color, DisplayItem, DisplayList, Pixmap};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event::{Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};

pub const TOOLBAR_HEIGHT: u32 = 36;

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

/// Re-runs the layout engine and rasterizer for the webpage given the viewport dimensions.
pub fn render_to_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    render_webpage_to_pixmap(doc, html_id, styles, width, height, 0.0).map(|(pixmap, _)| pixmap)
}

/// Re-runs the layout engine and rasterizer for the webpage with a vertical scroll offset.
/// Returns the rendered `Pixmap` and the `total_content_height`.
pub fn render_webpage_to_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    scroll_offset_y: f32,
) -> Option<(Pixmap, f32)> {
    let mut root_box = build_layout_tree(html_id, doc.arena(), styles)?;
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, width as f32, height as f32));
    layout(&mut root_box, viewport);

    let mut display_list = build_display_list(&root_box);
    let total_content_height = display_list
        .total_height()
        .max(root_box.dimensions.margin_box().height);

    if scroll_offset_y != 0.0 {
        display_list.offset(0.0, -scroll_offset_y);
    }

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
    Some((pixmap, total_content_height))
}

/// Renders the complete window frame including the address bar at the top and the webpage below it.
#[allow(clippy::too_many_arguments)]
pub fn render_window_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    current_url: &str,
    scroll_offset_y: &mut f32,
    total_content_height: &mut f32,
) -> Option<Pixmap> {
    let mut window_pixmap = Pixmap::new(width, height, Color::rgb(0x18, 0x18, 0x25));

    let page_height = height.saturating_sub(TOOLBAR_HEIGHT);
    if page_height > 0 && width > 0 {
        let (page_pixmap, content_h) = render_webpage_to_pixmap(
            doc,
            html_id,
            styles,
            width,
            page_height,
            *scroll_offset_y,
        )?;
        *total_content_height = content_h;
        let max_scroll = (content_h - page_height as f32).max(0.0);
        *scroll_offset_y = scroll_offset_y.clamp(0.0, max_scroll);

        window_pixmap.blit(&page_pixmap, 0, TOOLBAR_HEIGHT);
    } else {
        *total_content_height = 0.0;
        *scroll_offset_y = 0.0;
    }

    render_address_bar(&mut window_pixmap, current_url, width);

    Some(window_pixmap)
}

fn render_address_bar(pixmap: &mut Pixmap, current_url: &str, width: u32) {
    let mut bar_list = DisplayList::new();

    // 1. Toolbar background
    bar_list.push(DisplayItem::SolidColor {
        rect: Rect::new(0.0, 0.0, width as f32, TOOLBAR_HEIGHT as f32),
        color: Color::rgb(0x18, 0x18, 0x25),
    });

    // 2. Input box
    if width > 32 {
        let box_w = (width - 24) as f32;
        bar_list.push(DisplayItem::SolidColor {
            rect: Rect::new(12.0, 6.0, box_w, 24.0),
            color: Color::rgb(0x31, 0x32, 0x44),
        });
        bar_list.push(DisplayItem::Border {
            rect: Rect::new(12.0, 6.0, box_w, 24.0),
            widths: EdgeSizes::new(1.0, 1.0, 1.0, 1.0),
            color: Color::rgb(0x45, 0x47, 0x5a),
        });

        // URL text inside input box
        bar_list.push(DisplayItem::Text {
            text: current_url.to_string(),
            rect: Rect::new(20.0, 9.0, box_w - 16.0, 18.0),
            color: Color::rgb(0xcd, 0xd6, 0xf4),
            font_size: 13.0,
        });
    }

    // 3. Subtle bottom border separator
    bar_list.push(DisplayItem::SolidColor {
        rect: Rect::new(0.0, (TOOLBAR_HEIGHT - 1) as f32, width as f32, 1.0),
        color: Color::rgb(0x45, 0x47, 0x5a),
    });

    rasterize(&bar_list, pixmap);
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
#[allow(clippy::too_many_arguments)]
fn render_frame(
    surface: &mut Surface<Arc<Window>, Arc<Window>>,
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    current_url: &str,
    scroll_offset_y: &mut f32,
    total_content_height: &mut f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
        return Ok(());
    };

    surface.resize(w, h)?;

    let pixmap = render_window_pixmap(
        doc,
        html_id,
        styles,
        width,
        height,
        current_url,
        scroll_offset_y,
        total_content_height,
    )
    .ok_or("Failed to build layout tree from root")?;

    let mut buffer = surface.buffer_mut()?;
    copy_pixmap_to_softbuffer(&pixmap, &mut buffer);
    buffer.present()?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "=== lilies: Modern Browser Engine (Milestone 7 - Live Networking & Scrolling) ==="
    );

    // Step 1: Parse CLI argument or default to https://example.com
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://example.com".to_string());
    println!("Loading: {}", target);

    let (html_content, current_url) = match lilies_net::fetch(&target) {
        Ok(html) => {
            println!(
                "Successfully fetched {} bytes from '{}'",
                html.len(),
                target
            );
            (html, target)
        }
        Err(err) => {
            eprintln!(
                "Could not fetch '{}': {}. Falling back to default demo HTML.",
                target, err
            );
            (
                DEFAULT_HTML.to_string(),
                format!("{} (offline demo)", target),
            )
        }
    };

    // Step 2: Parse HTML and compute styles merging <style> tags and inline attributes
    let doc = parse_html(&html_content);
    let styles = compute_styles_for_document(&doc);

    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .or_else(|| {
            doc.get_children(doc.root())
                .into_iter()
                .find(|&id| doc.node(id).and_then(|n| n.as_element()).is_some())
        })
        .expect("Root <html> element required in document");

    println!(
        "Document parsed successfully (DOM nodes: {})",
        doc.arena().len()
    );

    // Step 3: Initialize EventLoop and Window titled "lilies"
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("lilies")
            .with_inner_size(LogicalSize::new(800.0, 600.0))
            .build(&event_loop)?,
    );

    // Step 4: Initialize softbuffer Context and Surface
    let context = Context::new(window.clone())?;
    let mut surface = Surface::new(&context, window.clone())?;

    let mut scroll_offset_y: f32 = 0.0;
    let mut total_content_height: f32 = 0.0;

    // Initial render with current window size
    let size = window.inner_size();
    if let Err(e) = render_frame(
        &mut surface,
        &doc,
        html_id,
        &styles,
        size.width,
        size.height,
        &current_url,
        &mut scroll_offset_y,
        &mut total_content_height,
    ) {
        eprintln!("Initial render warning: {e}");
    }

    // Step 5: Run event loop
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
                    &current_url,
                    &mut scroll_offset_y,
                    &mut total_content_height,
                ) {
                    eprintln!("Render error on resize: {e}");
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => -y * 36.0,
                    MouseScrollDelta::PixelDelta(pos) => -pos.y as f32,
                };
                let old_scroll = scroll_offset_y;
                let page_height = window
                    .inner_size()
                    .height
                    .saturating_sub(TOOLBAR_HEIGHT) as f32;
                let max_scroll = (total_content_height - page_height).max(0.0);
                scroll_offset_y = (scroll_offset_y + scroll_amount).clamp(0.0, max_scroll);
                if (scroll_offset_y - old_scroll).abs() > 0.001 {
                    window.request_redraw();
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
                    &current_url,
                    &mut scroll_offset_y,
                    &mut total_content_height,
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
    use lilies_css::{compute_styles, parse_styles_from_document, StyleSheet};

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

        let pixmap_800 = render_to_pixmap(&doc, html_id, &styles, 800, 600).expect("pixmap");
        assert_eq!(pixmap_800.width, 800);
        assert_eq!(pixmap_800.height, 600);

        let card_color = Color::rgba(0x2b, 0x3a, 0x4a, 0xff);
        let bg_color = Color::rgba(0xff, 0xff, 0xff, 0xff);

        // At y = 60 (inside card vertically, margin-top is 50):
        assert_eq!(pixmap_800.get_pixel(199, 60), Some(bg_color));
        assert_eq!(pixmap_800.get_pixel(200, 60), Some(card_color));
        assert_eq!(pixmap_800.get_pixel(599, 60), Some(card_color));
        assert_eq!(pixmap_800.get_pixel(600, 60), Some(bg_color));

        let pixmap_1000 = render_to_pixmap(&doc, html_id, &styles, 1000, 600).expect("pixmap");
        assert_eq!(pixmap_1000.width, 1000);
        assert_eq!(pixmap_1000.height, 600);

        assert_eq!(pixmap_1000.get_pixel(299, 60), Some(bg_color));
        assert_eq!(pixmap_1000.get_pixel(300, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(699, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(700, 60), Some(bg_color));

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
        let stylesheet = parse_styles_from_document(&doc);
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

        assert_eq!(pixmap.get_pixel(50, 50), Some(body_bg));
        assert_eq!(pixmap.get_pixel(80, 45), Some(container_bg));

        assert_eq!(pixmap.get_pixel(75, 50), Some(body_bg));
        assert_eq!(pixmap.get_pixel(76, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(723, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(724, 50), Some(body_bg));

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

        let mut root_800 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_800,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );

        let mut root_1200 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_1200,
            Dimensions::new(Rect::new(0.0, 0.0, 1200.0, 800.0)),
        );

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

        let mut wide_box =
            build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut wide_box,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );
        let wide_p = &wide_box.children[0].children[0];
        let wide_text = &wide_p.children[0];
        let wide_line_count = wide_text.lines.len();

        let mut narrow_box =
            build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut narrow_box,
            Dimensions::new(Rect::new(0.0, 0.0, 150.0, 600.0)),
        );
        let narrow_p = &narrow_box.children[0].children[0];
        let narrow_text = &narrow_p.children[0];
        let narrow_line_count = narrow_text.lines.len();

        assert!(narrow_line_count > wide_line_count);
        assert!(narrow_p.dimensions.content.height > wide_p.dimensions.content.height);
    }

    #[test]
    fn test_vertical_scrolling_offsets_page_content() {
        let html = r#"<html>
          <body style="background-color: #000000; margin: 0px; padding: 0px;">
            <div id="first" style="background-color: #ff0000; width: 400px; height: 200px;"></div>
            <div id="second" style="background-color: #0000ff; width: 400px; height: 200px;"></div>
          </body>
        </html>"#;

        let doc = parse_html(html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        // 1. Render at scroll_offset_y = 0:
        let (pixmap_top, total_h) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 0.0).expect("pixmap_top");
        assert_eq!(total_h, 400.0);

        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);

        // At y = 50, first box (red) is visible
        assert_eq!(pixmap_top.get_pixel(50, 50), Some(red));
        // At y = 250, second box (blue) is visible
        assert_eq!(pixmap_top.get_pixel(50, 250), Some(blue));

        // 2. Render scrolled down by 100px:
        let (pixmap_scrolled, _) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 100.0)
                .expect("pixmap_scrolled");

        // The first box was y in [0, 200), now offset by -100px -> y in [-100, 100).
        // The second box was y in [200, 400), now offset by -100px -> y in [100, 300).
        // At y = 50, red box is still there:
        assert_eq!(pixmap_scrolled.get_pixel(50, 50), Some(red));
        // At y = 150, blue box has scrolled up into view!
        assert_eq!(pixmap_scrolled.get_pixel(50, 150), Some(blue));

        // 3. Render scrolled down by 200px:
        let (pixmap_scrolled_200, _) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 200.0)
                .expect("pixmap_scrolled_200");

        // At y = 50, blue box is now at the top!
        assert_eq!(pixmap_scrolled_200.get_pixel(50, 50), Some(blue));
    }

    #[test]
    fn test_window_address_bar_rendering_and_dimensions() {
        let html = r#"<html><body style="background-color: #ffffff;"><p>Hello</p></body></html>"#;
        let doc = parse_html(html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut scroll_offset_y = 0.0;
        let mut total_content_h = 0.0;
        let url = "https://example.com";

        let window_pixmap = render_window_pixmap(
            &doc,
            html_id,
            &styles,
            800,
            600,
            url,
            &mut scroll_offset_y,
            &mut total_content_h,
        )
        .expect("window pixmap");

        assert_eq!(window_pixmap.width, 800);
        assert_eq!(window_pixmap.height, 600);

        // Top 36px is toolbar:
        // Background color of toolbar at x = 2, y = 2:
        assert_eq!(
            window_pixmap.get_pixel(2, 2),
            Some(Color::rgb(0x18, 0x18, 0x25))
        );

        // Separator line at y = TOOLBAR_HEIGHT - 1 (y = 35):
        assert_eq!(
            window_pixmap.get_pixel(100, TOOLBAR_HEIGHT - 1),
            Some(Color::rgb(0x45, 0x47, 0x5a))
        );

        // Below toolbar (y = 40): webpage background (white)
        assert_eq!(
            window_pixmap.get_pixel(100, TOOLBAR_HEIGHT + 4),
            Some(Color::WHITE)
        );
    }

    #[test]
    #[ignore]
    fn test_e2e_fetch_and_render_example_com() {
        let html = lilies_net::fetch("https://example.com").expect("fetch example.com");
        let doc = parse_html(&html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut scroll_offset_y = 0.0;
        let mut total_content_height = 0.0;
        let pixmap = render_window_pixmap(
            &doc,
            html_id,
            &styles,
            800,
            600,
            "https://example.com",
            &mut scroll_offset_y,
            &mut total_content_height,
        )
        .expect("render window pixmap");

        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);
        assert!(total_content_height > 0.0);

        let _ = pixmap.save_ppm("../../output_example_com.ppm");
    }
}
