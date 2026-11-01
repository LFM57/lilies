#![deny(unsafe_code)]

use lilies_css::{compute_styles, parse_stylesheet};
use lilies_html::parse_html;

fn main() {
    println!("=== lilies: Modern Browser Engine (Milestones 1 & 2) ===");

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

    println!("\nPipeline Status:");
    println!("  [x] HTML Tokenizer & Tree Builder (crates/lilies-html - Milestone 1)");
    println!("  [x] CSS Parser & Computed Styles (crates/lilies-css - Milestone 2)");
    println!("  [ ] Layout Engine (crates/lilies-layout - Milestone 3)");
    println!("  [ ] Display List & 2D Painter (crates/lilies-paint - Milestone 4)");
}
