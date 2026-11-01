#![deny(unsafe_code)]

use lilies_css::{compute_styles, parse_stylesheet};
use lilies_html::parse_html;
use lilies_layout::{build_layout_tree, layout, BoxType, Dimensions, LayoutBox, Rect};

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
    println!("=== lilies: Modern Browser Engine (Milestones 1, 2 & 3) ===");

    let html_input = r#"<!DOCTYPE html>
<html>
  <body>
    <div id="main" class="box">
      <span class="highlight">hello lilies</span>
    </div>
  </body>
</html>"#;

    let css_input = r#"
      div {
        display: block;
        margin: 10px;
        color: red;
      }
      .box {
        width: 200px;
        background-color: #f0f0f0;
      }
      #main {
        padding: 15px;
      }
      .highlight {
        color: blue;
      }
    "#;

    println!("\n--- 1. Input HTML ---");
    println!("{}", html_input);

    let doc = parse_html(html_input);

    println!("\n--- 2. Parsed DOM Tree ---");
    println!("{}", doc.dump_document());

    println!("\n--- 3. Input CSS ---");
    println!("{}", css_input.trim());

    let stylesheet = parse_stylesheet(css_input).expect("Failed to parse CSS");
    println!(
        "\n--- 4. Parsed Stylesheet ({} rules) ---",
        stylesheet.rules.len()
    );

    let styles = compute_styles(&doc, &stylesheet);
    println!("\n--- 5. Computed Styles Tree ---");
    for (node_id, style) in styles.iter() {
        if let Some(node) = doc.node(*node_id) {
            if let Some(el) = node.as_element() {
                println!(
                    "  {} <{}>: display={:?}, color={}, bg={}, width={:?}, margin={:?}, padding={}",
                    node_id,
                    el.tag_name(),
                    style.display,
                    style.color,
                    style.background_color,
                    style.width,
                    style.margin_top,
                    style.padding_top,
                );
            }
        }
    }

    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    });

    if let Some(html_id) = html_id {
        if let Some(mut root_box) = build_layout_tree(html_id, doc.arena(), &styles) {
            let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
            layout(&mut root_box, viewport);
            println!("\n--- 6. Computed Layout Tree ---");
            print_layout_tree(&root_box, 0);
        }
    }

    println!("\nPipeline Status:");
    println!("  [x] HTML Tokenizer & Tree Builder (crates/lilies-html - Milestone 1)");
    println!("  [x] CSS Parser & Computed Styles (crates/lilies-css - Milestone 2)");
    println!("  [x] Layout Engine (crates/lilies-layout - Milestone 3)");
    println!("  [ ] Display List & 2D Painter (crates/lilies-paint - Milestone 4)");
}
