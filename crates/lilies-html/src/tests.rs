#![deny(unsafe_code)]

use crate::{parse_html, Token, Tokenizer};
use indexmap::IndexMap;

#[test]
fn test_tokenize_simple_html() {
    let input = "<html><body><div class=\"box\">hello</div></body></html>";
    let tokenizer = Tokenizer::new(input);
    let tokens: Vec<Token> = tokenizer.collect();

    let mut expected_div_attrs = IndexMap::new();
    expected_div_attrs.insert("class".to_string(), "box".to_string());

    let expected = vec![
        Token::StartTag {
            name: "html".to_string(),
            attributes: IndexMap::new(),
            self_closing: false,
        },
        Token::StartTag {
            name: "body".to_string(),
            attributes: IndexMap::new(),
            self_closing: false,
        },
        Token::StartTag {
            name: "div".to_string(),
            attributes: expected_div_attrs,
            self_closing: false,
        },
        Token::Character('h'),
        Token::Character('e'),
        Token::Character('l'),
        Token::Character('l'),
        Token::Character('o'),
        Token::EndTag {
            name: "div".to_string(),
        },
        Token::EndTag {
            name: "body".to_string(),
        },
        Token::EndTag {
            name: "html".to_string(),
        },
        Token::Eof,
    ];

    assert_eq!(tokens, expected);
}

#[test]
fn test_parse_tree_creation_for_required_input() {
    let input = "<html><body><div class=\"box\">hello</div></body></html>";
    let doc = parse_html(input);

    let dumped = doc.dump_document();
    let expected = "\
#document
  <html>
    <body>
      <div class=\"box\">
        #text \"hello\"";

    assert_eq!(dumped, expected);

    // Deep inspect the DOM structure
    let root = doc.root();
    let root_children = doc.get_children(root);
    assert_eq!(root_children.len(), 1);

    let html_id = root_children[0];
    let html_node = doc.node(html_id).unwrap();
    assert_eq!(html_node.as_element().unwrap().tag_name(), "html");

    let body_id = doc.first_child(html_id).unwrap();
    let body_node = doc.node(body_id).unwrap();
    assert_eq!(body_node.as_element().unwrap().tag_name(), "body");

    let div_id = doc.first_child(body_id).unwrap();
    let div_node = doc.node(div_id).unwrap();
    let div_el = div_node.as_element().unwrap();
    assert_eq!(div_el.tag_name(), "div");
    assert_eq!(div_el.get_attribute("class"), Some("box"));

    let text_id = doc.first_child(div_id).unwrap();
    let text_node = doc.node(text_id).unwrap();
    assert_eq!(text_node.as_text().unwrap().data(), "hello");
}

#[test]
fn test_tokenize_attributes_variations() {
    let input = "<input id=\"user\" class='text-input' checked name=login>";
    let tokenizer = Tokenizer::new(input);
    let tokens: Vec<Token> = tokenizer.collect();

    assert_eq!(tokens.len(), 2); // StartTag + Eof
    if let Token::StartTag {
        name, attributes, ..
    } = &tokens[0]
    {
        assert_eq!(name, "input");
        assert_eq!(attributes.get("id"), Some(&"user".to_string()));
        assert_eq!(attributes.get("class"), Some(&"text-input".to_string()));
        assert_eq!(attributes.get("checked"), Some(&"".to_string()));
        assert_eq!(attributes.get("name"), Some(&"login".to_string()));
    } else {
        panic!("Expected StartTag token");
    }
}

#[test]
fn test_doctype_and_comment_parsing() {
    let input = "<!DOCTYPE html><!-- navigation --><div>nav</div>";
    let doc = parse_html(input);

    let dumped = doc.dump_document();
    let expected = "\
#document
  <!DOCTYPE html>
  <!-- navigation -->
  <div>
    #text \"nav\"";

    assert_eq!(dumped, expected);
}

#[test]
fn test_void_and_self_closing_elements() {
    let input = "<div><img src=\"logo.png\"><br/><p>Text</p></div>";
    let doc = parse_html(input);

    let dumped = doc.dump_document();
    let expected = "\
#document
  <div>
    <img src=\"logo.png\">
    <br>
    <p>
      #text \"Text\"";

    assert_eq!(dumped, expected);
}

#[test]
fn test_style_tag_extraction_and_application() {
    let html = r#"
        <html>
          <head>
            <style>
              body { background-color: #ffffff; color: #111111; }
              h1 { color: #ff0000; font-size: 32px; }
              p.intro { color: #0000ff; margin: 10px; }
            </style>
          </head>
          <body>
            <h1>Title</h1>
            <p class="intro" style="color: #00ff00;">Green text</p>
            <p>Normal text</p>
          </body>
        </html>
    "#;

    let doc = parse_html(html);
    let style_text = crate::extract_style_text(&doc);
    assert!(style_text.contains("h1 { color: #ff0000; font-size: 32px; }"));
    assert!(style_text.contains("p.intro { color: #0000ff; margin: 10px; }"));

    let styles = crate::compute_styles_for_document(&doc);

    // Find body, h1, and p.intro elements
    let root = doc.root();
    let mut elements = Vec::new();
    let mut stack = vec![root];
    while let Some(node_id) = stack.pop() {
        if let Some(node) = doc.node(node_id) {
            if let Some(el) = node.as_element() {
                elements.push((
                    node_id,
                    el.tag_name().to_string(),
                    el.get_attribute("class").map(|s| s.to_string()),
                ));
            }
        }
        for child in doc.get_children(node_id) {
            stack.push(child);
        }
    }

    let h1_id = elements
        .iter()
        .find(|(_, tag, _)| tag == "h1")
        .unwrap()
        .0;
    let h1_style = styles.get(h1_id).expect("h1 style");
    assert_eq!(h1_style.color, lilies_css::Color::rgb(255, 0, 0));
    assert_eq!(h1_style.font_size, 32.0);

    let intro_id = elements
        .iter()
        .find(|(_, tag, class)| tag == "p" && class.as_deref() == Some("intro"))
        .unwrap()
        .0;
    let intro_style = styles.get(intro_id).expect("p.intro style");
    // Inline style overrides style tag: color is green (not blue)
    assert_eq!(intro_style.color, lilies_css::Color::rgb(0, 255, 0));
    // Margin is preserved from style tag
    assert_eq!(intro_style.margin_top, lilies_css::LengthOrAuto::Px(10.0));
}

#[test]
fn test_multiple_style_blocks_and_cascade_order() {
    let html = r#"
        <html>
          <head>
            <style>
              div.box { background-color: #ff0000; width: 100px; }
            </style>
          </head>
          <body>
            <style>
              div.box { background-color: #0000ff; }
            </style>
            <div class="box"></div>
          </body>
        </html>
    "#;

    let doc = parse_html(html);
    let styles = crate::compute_styles_for_document(&doc);

    let div_id = doc
        .get_children(doc.root())
        .into_iter()
        .flat_map(|id| doc.get_children(id))
        .flat_map(|id| doc.get_children(id))
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "div")
        })
        .unwrap();

    let div_style = styles.get(div_id).unwrap();
    // Later <style> block in body wins over earlier <style> block in head
    assert_eq!(div_style.background_color, lilies_css::Color::rgb(0, 0, 255));
    // Property from earlier block is preserved
    assert_eq!(div_style.width, lilies_css::LengthOrAuto::Px(100.0));
}
