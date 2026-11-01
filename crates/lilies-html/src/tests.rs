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
