#![deny(unsafe_code)]

use crate::{build_display_list, rasterize, Color, DisplayItem, DisplayList, Pixmap};
use lilies_css::{compute_styles, ComputedStyle, StyleSheet};
use lilies_dom::NodeId;
use lilies_html::parse_html;
use lilies_layout::{build_layout_tree, layout, BoxType, Dimensions, EdgeSizes, LayoutBox, Rect};
use std::fs;

#[test]
fn test_display_list_generation_nested_boxes() {
    let mut parent = LayoutBox::new(BoxType::BlockNode(NodeId(1)));
    parent.dimensions = Dimensions {
        content: Rect::new(50.0, 50.0, 200.0, 100.0),
        padding: EdgeSizes::all(10.0),
        border: EdgeSizes::all(5.0),
        margin: EdgeSizes::all(0.0),
    };
    parent.style = Some(ComputedStyle {
        background_color: Color::rgb(40, 50, 60),
        color: Color::rgb(10, 20, 30),
        ..Default::default()
    });

    let mut child = LayoutBox::new(BoxType::BlockNode(NodeId(2)));
    child.dimensions = Dimensions {
        content: Rect::new(70.0, 70.0, 100.0, 50.0),
        padding: EdgeSizes::all(5.0),
        border: EdgeSizes::all(2.0),
        margin: EdgeSizes::all(0.0),
    };
    child.style = Some(ComputedStyle {
        background_color: Color::rgb(255, 80, 0),
        color: Color::BLACK,
        ..Default::default()
    });
    parent.children.push(child);

    let display_list = build_display_list(&parent);

    // CSS 2.1 paint order:
    // 1. Parent background
    // 2. Parent borders
    // 3. Child background
    // 4. Child borders
    assert_eq!(display_list.len(), 4);

    // Item 0: Parent background (padding box)
    // padding box of parent: content(50, 50, 200, 100) + padding 10 => (40, 40, 220, 120)
    assert_eq!(
        display_list.items[0],
        DisplayItem::SolidColor {
            rect: Rect::new(40.0, 40.0, 220.0, 120.0),
            color: Color::rgb(40, 50, 60),
        }
    );

    // Item 1: Parent border (border box)
    // border box of parent: padding_box(40, 40, 220, 120) + border 5 => (35, 35, 230, 130)
    assert_eq!(
        display_list.items[1],
        DisplayItem::Border {
            rect: Rect::new(35.0, 35.0, 230.0, 130.0),
            widths: EdgeSizes::all(5.0),
            color: Color::rgb(10, 20, 30),
        }
    );

    // Item 2: Child background (padding box)
    // padding box of child: content(70, 70, 100, 50) + padding 5 => (65, 65, 110, 60)
    assert_eq!(
        display_list.items[2],
        DisplayItem::SolidColor {
            rect: Rect::new(65.0, 65.0, 110.0, 60.0),
            color: Color::rgb(255, 80, 0),
        }
    );

    // Item 3: Child border (border box)
    // border box of child: padding_box(65, 65, 110, 60) + border 2 => (63, 63, 114, 64)
    assert_eq!(
        display_list.items[3],
        DisplayItem::Border {
            rect: Rect::new(63.0, 63.0, 114.0, 64.0),
            widths: EdgeSizes::all(2.0),
            color: Color::BLACK,
        }
    );
}

#[test]
fn test_display_list_transparent_background_skipped() {
    let mut layout_box = LayoutBox::new(BoxType::AnonymousBlock);
    layout_box.dimensions = Dimensions {
        content: Rect::new(0.0, 0.0, 100.0, 50.0),
        padding: EdgeSizes::default(),
        border: EdgeSizes::default(),
        margin: EdgeSizes::default(),
    };
    layout_box.style = Some(ComputedStyle {
        background_color: Color::TRANSPARENT,
        ..Default::default()
    });

    let display_list = build_display_list(&layout_box);
    assert!(display_list.is_empty());
}

#[test]
fn test_rasterizer_pixel_colors_and_layout() {
    let mut pixmap = Pixmap::new(100, 100, Color::WHITE);
    let mut display_list = DisplayList::new();

    // Draw blue rectangle (10, 10) to (50, 50)
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(10.0, 10.0, 40.0, 40.0),
        color: Color::rgb(0, 0, 255),
    });

    // Draw red rectangle (20, 20) to (40, 40) on top of blue
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(20.0, 20.0, 20.0, 20.0),
        color: Color::rgb(255, 0, 0),
    });

    rasterize(&display_list, &mut pixmap);

    // Outside blue rectangle: white
    assert_eq!(pixmap.get_pixel(5, 5), Some(Color::WHITE));
    assert_eq!(pixmap.get_pixel(60, 60), Some(Color::WHITE));

    // Blue area outside red inner box: blue
    assert_eq!(pixmap.get_pixel(15, 15), Some(Color::rgb(0, 0, 255)));
    assert_eq!(pixmap.get_pixel(45, 45), Some(Color::rgb(0, 0, 255)));

    // Red inner box: red
    assert_eq!(pixmap.get_pixel(25, 25), Some(Color::rgb(255, 0, 0)));
    assert_eq!(pixmap.get_pixel(35, 35), Some(Color::rgb(255, 0, 0)));
}

#[test]
fn test_rasterizer_border_rendering() {
    let mut pixmap = Pixmap::new(100, 100, Color::WHITE);
    let mut display_list = DisplayList::new();

    display_list.push(DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 80.0, 60.0),
        widths: EdgeSizes::new(2.0, 3.0, 4.0, 5.0),
        color: Color::BLACK,
    });

    rasterize(&display_list, &mut pixmap);

    // Outside the border: white
    assert_eq!(pixmap.get_pixel(5, 5), Some(Color::WHITE));

    // Top border (y in 10..12): black
    assert_eq!(pixmap.get_pixel(50, 10), Some(Color::BLACK));
    assert_eq!(pixmap.get_pixel(50, 11), Some(Color::BLACK));

    // Left border (x in 10..15): black
    assert_eq!(pixmap.get_pixel(12, 30), Some(Color::BLACK));

    // Right border (x in 87..90): black
    assert_eq!(pixmap.get_pixel(88, 30), Some(Color::BLACK));

    // Bottom border (y in 66..70): black
    assert_eq!(pixmap.get_pixel(50, 68), Some(Color::BLACK));

    // Interior of the box (e.g. x: 30, y: 30): white!
    assert_eq!(pixmap.get_pixel(30, 30), Some(Color::WHITE));
}

#[test]
fn test_rasterizer_alpha_blending() {
    let mut pixmap = Pixmap::new(10, 10, Color::WHITE);
    let mut display_list = DisplayList::new();

    // 50% opacity black over white
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        color: Color::rgba(0, 0, 0, 128),
    });

    rasterize(&display_list, &mut pixmap);

    let pixel = pixmap.get_pixel(5, 5).unwrap();
    // 0 * 0.5 + 255 * 0.5 ≈ 127
    assert!(pixel.r >= 126 && pixel.r <= 128);
    assert!(pixel.g >= 126 && pixel.g <= 128);
    assert!(pixel.b >= 126 && pixel.b <= 128);
    assert_eq!(pixel.a, 255);
}

#[test]
fn test_viewport_clipping_does_not_panic_or_corrupt() {
    let mut pixmap = Pixmap::new(50, 50, Color::BLACK);
    let mut display_list = DisplayList::new();

    // Completely off-screen top-left
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(-200.0, -200.0, 100.0, 100.0),
        color: Color::RED,
    });

    // Partially off-screen top-left
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(-20.0, -20.0, 30.0, 30.0),
        color: Color::BLUE,
    });

    // Completely off-screen bottom-right
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(100.0, 100.0, 50.0, 50.0),
        color: Color::GREEN,
    });

    // Partially off-screen bottom-right
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(40.0, 40.0, 20.0, 20.0),
        color: Color::YELLOW,
    });

    // Negative dimensions
    display_list.push(DisplayItem::SolidColor {
        rect: Rect::new(10.0, 10.0, -50.0, 20.0),
        color: Color::RED,
    });

    rasterize(&display_list, &mut pixmap);

    // Partially drawn blue pixel at (5, 5)
    assert_eq!(pixmap.get_pixel(5, 5), Some(Color::BLUE));

    // Partially drawn yellow pixel at (45, 45)
    assert_eq!(pixmap.get_pixel(45, 45), Some(Color::YELLOW));

    // Untouched middle pixel at (25, 25) remains black
    assert_eq!(pixmap.get_pixel(25, 25), Some(Color::BLACK));
}

#[test]
fn test_ppm_export() {
    let mut pixmap = Pixmap::new(4, 4, Color::WHITE);
    pixmap.set_pixel(0, 0, Color::RED);

    let temp_path = std::env::temp_dir().join("lilies_test_output.ppm");
    pixmap.save_ppm(&temp_path).expect("Failed to save PPM");

    let bytes = fs::read(&temp_path).expect("Failed to read PPM back");
    let _ = fs::remove_file(&temp_path);

    // Check header
    let header = b"P6\n4 4\n255\n";
    assert!(bytes.starts_with(header));

    // Check pixel payload size: 4 * 4 * 3 = 48 bytes
    assert_eq!(bytes.len(), header.len() + 48);

    // First pixel should be RED (255, 0, 0)
    let payload = &bytes[header.len()..];
    assert_eq!(payload[0], 255);
    assert_eq!(payload[1], 0);
    assert_eq!(payload[2], 0);

    // Second pixel should be WHITE (255, 255, 255)
    assert_eq!(payload[3], 255);
    assert_eq!(payload[4], 255);
    assert_eq!(payload[5], 255);
}

#[test]
fn test_full_pipeline_html_to_display_list_to_pixmap() {
    let html = r#"<html>
  <body style="background-color: #ffffff;">
    <div class="card" style="background-color: #2b3a4a; width: 400px; height: 300px; margin: 50px auto;">
      <div class="inner" style="background-color: #ff5722; width: 200px; height: 100px; margin: 20px;"></div>
    </div>
  </body>
</html>"#;

    let doc = parse_html(html);
    let stylesheet = StyleSheet::new();
    let style_tree = compute_styles(&doc, &stylesheet);

    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    });

    let mut root_box = build_layout_tree(html_id.unwrap(), doc.arena(), &style_tree).unwrap();
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    layout(&mut root_box, viewport);

    let display_list = build_display_list(&root_box);
    // Body, Card, and Inner should each generate a SolidColor item
    assert_eq!(display_list.len(), 3);

    let mut pixmap = Pixmap::new(800, 600, Color::WHITE);
    rasterize(&display_list, &mut pixmap);

    // Card is 400px wide, centered in 800px viewport:
    // x = (800 - 400) / 2 = 200.
    // Margin top = 50.
    // Body has background #ffffff.
    // Card has background #2b3a4a (rgb(43, 58, 74)).
    // Inside card, inner box starts at (200 + 20, 50 + 20) = (220, 70), size 200x100.
    // Inner box has background #ff5722 (rgb(255, 87, 34)).

    // Center of inner box: (320, 120) -> #ff5722
    assert_eq!(pixmap.get_pixel(320, 120), Some(Color::rgb(255, 87, 34)));

    // Inside card but outside inner box: (210, 60) -> #2b3a4a
    assert_eq!(pixmap.get_pixel(210, 60), Some(Color::rgb(43, 58, 74)));

    // Outside card: (50, 50) -> #ffffff
    assert_eq!(pixmap.get_pixel(50, 50), Some(Color::WHITE));
}

#[test]
fn test_glyph_rasterization_basic() {
    let mut pixmap = Pixmap::new(200, 60, Color::WHITE);
    let mut display_list = DisplayList::new();
    display_list.push(DisplayItem::Text {
        text: "lilies".to_string(),
        rect: Rect::new(10.0, 10.0, 100.0, 30.0),
        color: Color::BLACK,
        font_size: 24.0,
    });

    rasterize(&display_list, &mut pixmap);

    // Verify some non-white pixels were drawn for the text glyphs
    let black_or_gray_pixels = (0..60)
        .flat_map(|y| (0..200).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = pixmap.get_pixel(x, y).unwrap();
            p != Color::WHITE
        })
        .count();

    assert!(
        black_or_gray_pixels > 50,
        "Expected at least 50 rasterized text pixels, found {}",
        black_or_gray_pixels
    );
}

#[test]
fn test_glyph_rasterization_alpha_blending() {
    let mut pixmap = Pixmap::new(100, 50, Color::rgb(0, 0, 255)); // Blue background
    let mut display_list = DisplayList::new();

    // Semi-transparent red text (50% alpha)
    display_list.push(DisplayItem::Text {
        text: "H".to_string(),
        rect: Rect::new(10.0, 10.0, 50.0, 30.0),
        color: Color::rgba(255, 0, 0, 128),
        font_size: 24.0,
    });

    rasterize(&display_list, &mut pixmap);

    // Find glyph pixels that have blended red and blue
    let blended_pixels = (0..50)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = pixmap.get_pixel(x, y).unwrap();
            // Blended pixel should have both Red > 0 and Blue > 0
            p.r > 0 && p.b > 0
        })
        .count();

    assert!(
        blended_pixels > 10,
        "Expected blended glyph pixels with both R and B, found {}",
        blended_pixels
    );
}

#[test]
fn test_rasterization_out_of_bounds_clipping() {
    let mut pixmap = Pixmap::new(50, 50, Color::WHITE);
    let mut display_list = DisplayList::new();

    // Draw off-screen left and top
    display_list.push(DisplayItem::Text {
        text: "Negative Offset".to_string(),
        rect: Rect::new(-50.0, -20.0, 100.0, 30.0),
        color: Color::BLACK,
        font_size: 16.0,
    });

    // Draw off-screen right and bottom
    display_list.push(DisplayItem::Text {
        text: "Far Away".to_string(),
        rect: Rect::new(1000.0, 1000.0, 100.0, 30.0),
        color: Color::BLACK,
        font_size: 16.0,
    });

    // Draw spanning across the border edge
    display_list.push(DisplayItem::Text {
        text: "Crossing Viewport Edge".to_string(),
        rect: Rect::new(30.0, 30.0, 200.0, 30.0),
        color: Color::BLACK,
        font_size: 16.0,
    });

    // Must not panic or segfault or corrupt memory
    rasterize(&display_list, &mut pixmap);

    // Empty pixmap should also not panic
    let mut empty_pixmap = Pixmap::new(0, 0, Color::WHITE);
    rasterize(&display_list, &mut empty_pixmap);
}

#[test]
fn test_display_list_text_generation() {
    use lilies_layout::TextLine;

    let mut text_box = LayoutBox::new(BoxType::InlineNode(NodeId(5)));
    text_box.style = Some(ComputedStyle {
        color: Color::rgb(137, 180, 250),
        font_size: 32.0,
        ..Default::default()
    });
    text_box.lines = vec![
        TextLine {
            text: "Line 1".to_string(),
            rect: Rect::new(10.0, 10.0, 80.0, 38.4),
        },
        TextLine {
            text: "Line 2".to_string(),
            rect: Rect::new(10.0, 48.4, 80.0, 38.4),
        },
    ];

    let display_list = build_display_list(&text_box);
    assert_eq!(display_list.len(), 2);
    assert_eq!(
        display_list.items[0],
        DisplayItem::Text {
            text: "Line 1".to_string(),
            rect: Rect::new(10.0, 10.0, 80.0, 38.4),
            color: Color::rgb(137, 180, 250),
            font_size: 32.0,
        }
    );
    assert_eq!(
        display_list.items[1],
        DisplayItem::Text {
            text: "Line 2".to_string(),
            rect: Rect::new(10.0, 48.4, 80.0, 38.4),
            color: Color::rgb(137, 180, 250),
            font_size: 32.0,
        }
    );
}
