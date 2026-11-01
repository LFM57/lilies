#![deny(unsafe_code)]

use lilies_html::parse_html;

fn main() {
    println!("=== lilies: Modern Browser Engine (Milestone 1) ===");

    let html_input = r#"<!DOCTYPE html>
<html>
  <head>
    <title>Lilies Engine</title>
  </head>
  <body>
    <div class="box">hello</div>
  </body>
</html>"#;

    println!("\nInput HTML:\n{}", html_input);

    let doc = parse_html(html_input);

    println!("\nParsed DOM Tree:");
    println!("{}", doc.dump_document());

    println!("\nPipeline Status:");
    println!("  [x] HTML Tokenizer (WHATWG state machine)");
    println!("  [x] DOM Tree Construction (Arena-backed)");
    println!("  [ ] CSSOM Parser (crates/lilies-css - Milestone 2)");
    println!("  [ ] Layout Engine (crates/lilies-layout - Milestone 3)");
    println!("  [ ] Display List & 2D Painter (crates/lilies-paint - Milestone 4)");
}
