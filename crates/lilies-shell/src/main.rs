#![deny(unsafe_code)]

use lilies_css::{compute_styles, StyleSheet};
use lilies_html::parse_html;
use lilies_layout::{build_layout_tree, layout, BoxType, Dimensions, LayoutBox, Rect};
use lilies_paint::{build_display_list, rasterize, Color, DisplayItem, Pixmap};
use std::time::Instant;

fn count_layout_nodes(layout_box: &LayoutBox) -> usize {
    1 + layout_box
        .children
        .iter()
        .map(count_layout_nodes)
        .sum::<usize>()
}

fn print_layout_tree(layout_box: &LayoutBox, indent: usize) {
    let pad = "  ".repeat(indent);
    let box_desc = match layout_box.box_type {
        BoxType::BlockNode(id) => format!("BlockNode({})", id),
        BoxType::InlineNode(id) => format!("InlineNode({})", id),
        BoxType::AnonymousBlock => "AnonymousBlock".to_string(),
    };
    let c = &layout_box.dimensions.content;
    let m = &layout_box.dimensions.margin;
    let p = &layout_box.dimensions.padding;
    println!(
        "{}* {} content: ({:.1}, {:.1}, {:.1}x{:.1}) margin: [{:.1}, {:.1}, {:.1}, {:.1}] padding: [{:.1}, {:.1}, {:.1}, {:.1}]",
        pad, box_desc, c.x, c.y, c.width, c.height, m.top, m.right, m.bottom, m.left, p.top, p.right, p.bottom, p.left
    );
    for child in &layout_box.children {
        print_layout_tree(child, indent + 1);
    }
}

fn main() {
    println!("=== lilies: Modern Browser Engine (Milestones 1, 2, 3 & 4) ===");

    let html_input = r#"<html>
  <body style="background-color: #ffffff;">
    <div class="card" style="background-color: #2b3a4a; width: 400px; height: 300px; margin: 50px auto;">
      <div class="inner" style="background-color: #ff5722; width: 200px; height: 100px; margin: 20px;"></div>
    </div>
  </body>
</html>"#;

    println!("\n--- 1. Input HTML Document ---");
    println!("{}", html_input);

    let start_total = Instant::now();

    // Step 1: HTML Parsing
    let t0 = Instant::now();
    let doc = parse_html(html_input);
    let html_dur = t0.elapsed();
    println!(
        "\n[1/5] HTML Parsed: {} DOM nodes (took {:?})",
        doc.arena().len(),
        html_dur
    );

    // Step 2: CSS & Style Computation
    let t1 = Instant::now();
    let stylesheet = StyleSheet::new();
    let styles = compute_styles(&doc, &stylesheet);
    let css_dur = t1.elapsed();
    println!(
        "[2/5] Styles Computed: {} styled elements (took {:?})",
        styles.len(),
        css_dur
    );

    // Step 3: Layout Tree Construction & Block Layout
    let t2 = Instant::now();
    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("html element required");

    let mut root_box =
        build_layout_tree(html_id, doc.arena(), &styles).expect("Failed to build layout tree");
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    layout(&mut root_box, viewport);
    let layout_dur = t2.elapsed();
    let layout_nodes = count_layout_nodes(&root_box);
    println!(
        "[3/5] Layout Computed: {} layout boxes, viewport 800x600 (took {:?})",
        layout_nodes, layout_dur
    );

    println!("\n--- Layout Tree ---");
    print_layout_tree(&root_box, 0);

    // Step 4: Display List Construction
    let t3 = Instant::now();
    let display_list = build_display_list(&root_box);
    let paint_dur = t3.elapsed();
    println!(
        "\n[4/5] Display List Built: {} display items (took {:?})",
        display_list.len(),
        paint_dur
    );
    for (i, item) in display_list.iter().enumerate() {
        match item {
            DisplayItem::SolidColor { rect, color } => {
                println!(
                    "  [{}] SolidColor: ({:.1}, {:.1}, {:.1}x{:.1}) color={}",
                    i, rect.x, rect.y, rect.width, rect.height, color
                );
            }
            DisplayItem::Border {
                rect,
                widths,
                color,
            } => {
                println!(
                    "  [{}] Border: ({:.1}, {:.1}, {:.1}x{:.1}) widths=[{:.1}, {:.1}, {:.1}, {:.1}] color={}",
                    i, rect.x, rect.y, rect.width, rect.height, widths.top, widths.right, widths.bottom, widths.left, color
                );
            }
        }
    }

    // Step 5: Rasterization & Output
    let t4 = Instant::now();
    let mut pixmap = Pixmap::new(800, 600, Color::WHITE);
    rasterize(&display_list, &mut pixmap);
    let output_file = "output.ppm";
    pixmap
        .save_ppm(output_file)
        .expect("Failed to save output PPM");
    let raster_dur = t4.elapsed();
    println!(
        "[5/5] Rasterized & Exported: saved to '{}' (took {:?})",
        output_file, raster_dur
    );

    let total_dur = start_total.elapsed();
    println!("\nComplete Render Pipeline Finished in {:?}", total_dur);

    println!("\nPipeline Status:");
    println!("  [x] HTML Tokenizer & Tree Builder (crates/lilies-html - Milestone 1)");
    println!("  [x] CSS Parser & Computed Styles (crates/lilies-css - Milestone 2)");
    println!("  [x] Layout Engine (crates/lilies-layout - Milestone 3)");
    println!("  [x] Display List & 2D Painter (crates/lilies-paint - Milestone 4)");
}
