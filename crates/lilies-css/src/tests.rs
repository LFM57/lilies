#![deny(unsafe_code)]

use crate::{
    compute_styles, matches, parse_stylesheet, AlignItems, Color, Display, FlexDirection,
    JustifyContent, LengthOrAuto, Specificity,
};
use lilies_dom::{Document, IndexMap};

#[test]
fn test_css_parser_valid_syntax_and_comments() {
    let css = r#"
        /* Header Comment */
        div.box, #main {
            display: block; /* inline comment */
            width: 100px;
            margin: 10px 20px;
            padding: 5px;
            background-color: #ff0000;
            color: red;
        }

        /* Second Rule */
        span {
            display: none;
            color: rgba(0, 128, 255, 1.0);
        }
    "#;

    let stylesheet = parse_stylesheet(css).expect("Failed to parse valid CSS");
    assert_eq!(stylesheet.rules.len(), 2);

    // Rule 1
    let r1 = &stylesheet.rules[0];
    assert_eq!(r1.selectors.len(), 2);

    let s1 = &r1.selectors[0];
    assert_eq!(s1.tag_name, Some("div".to_string()));
    assert_eq!(s1.classes, vec!["box".to_string()]);
    assert_eq!(s1.id, None);

    let s2 = &r1.selectors[1];
    assert_eq!(s2.tag_name, None);
    assert_eq!(s2.id, Some("main".to_string()));
    assert!(s2.classes.is_empty());

    assert_eq!(r1.declarations.len(), 6);
    assert_eq!(r1.declarations[0].name, "display");
    assert_eq!(r1.declarations[1].name, "width");
    assert_eq!(r1.declarations[2].name, "margin");
    assert_eq!(r1.declarations[3].name, "padding");
    assert_eq!(r1.declarations[4].name, "background-color");
    assert_eq!(r1.declarations[5].name, "color");

    // Rule 2
    let r2 = &stylesheet.rules[1];
    assert_eq!(r2.selectors.len(), 1);
    assert_eq!(r2.selectors[0].tag_name, Some("span".to_string()));
    assert_eq!(r2.declarations.len(), 2);
    assert_eq!(
        r2.declarations[1].value.to_color(),
        Some(Color::rgba(0, 128, 255, 255))
    );
}

#[test]
fn test_specificity_calculation() {
    let css = r#"
        div { color: black; }
        .box { color: black; }
        div.box { color: black; }
        #main { color: black; }
        div#main.box { color: black; }
    "#;

    let stylesheet = parse_stylesheet(css).unwrap();
    let specs: Vec<Specificity> = stylesheet
        .rules
        .iter()
        .map(|r| r.selectors[0].specificity())
        .collect();

    assert_eq!(specs[0], Specificity(0, 0, 1)); // div
    assert_eq!(specs[1], Specificity(0, 1, 0)); // .box
    assert_eq!(specs[2], Specificity(0, 1, 1)); // div.box
    assert_eq!(specs[3], Specificity(1, 0, 0)); // #main
    assert_eq!(specs[4], Specificity(1, 1, 1)); // div#main.box

    // Ordering comparisons
    assert!(specs[4] > specs[3]); // (1, 1, 1) > (1, 0, 0)
    assert!(specs[3] > specs[2]); // (1, 0, 0) > (0, 1, 1)
    assert!(specs[2] > specs[1]); // (0, 1, 1) > (0, 1, 0)
    assert!(specs[1] > specs[0]); // (0, 1, 0) > (0, 0, 1)
}

#[test]
fn test_selector_matching() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut attrs = IndexMap::new();
    attrs.insert("id".to_string(), "main".to_string());
    attrs.insert("class".to_string(), "box container".to_string());
    let div = doc.create_element("div", attrs);
    doc.append_child(root, div).unwrap();

    let css = r#"
        div {}
        .box {}
        div.box {}
        #main {}
        div#main.box {}
        span {}
        .other {}
        #other {}
        div.other {}
    "#;
    let stylesheet = parse_stylesheet(css).unwrap();

    // Matches
    assert!(matches(&stylesheet.rules[0].selectors[0], div, doc.arena())); // div
    assert!(matches(&stylesheet.rules[1].selectors[0], div, doc.arena())); // .box
    assert!(matches(&stylesheet.rules[2].selectors[0], div, doc.arena())); // div.box
    assert!(matches(&stylesheet.rules[3].selectors[0], div, doc.arena())); // #main
    assert!(matches(&stylesheet.rules[4].selectors[0], div, doc.arena())); // div#main.box

    // Non-matches
    assert!(!matches(
        &stylesheet.rules[5].selectors[0],
        div,
        doc.arena()
    )); // span
    assert!(!matches(
        &stylesheet.rules[6].selectors[0],
        div,
        doc.arena()
    )); // .other
    assert!(!matches(
        &stylesheet.rules[7].selectors[0],
        div,
        doc.arena()
    )); // #other
    assert!(!matches(
        &stylesheet.rules[8].selectors[0],
        div,
        doc.arena()
    )); // div.other
}

#[test]
fn test_cascade_and_inheritance() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut parent_attrs = IndexMap::new();
    parent_attrs.insert("class".to_string(), "parent".to_string());
    let parent = doc.create_element("div", parent_attrs);
    doc.append_child(root, parent).unwrap();

    let mut child1_attrs = IndexMap::new();
    child1_attrs.insert("class".to_string(), "child1".to_string());
    let child1 = doc.create_element("span", child1_attrs);
    doc.append_child(parent, child1).unwrap();

    let mut child2_attrs = IndexMap::new();
    child2_attrs.insert("class".to_string(), "child2".to_string());
    let child2 = doc.create_element("span", child2_attrs);
    doc.append_child(parent, child2).unwrap();

    let css = r#"
        .parent {
            color: red;
            margin: 10px;
            width: 200px;
        }
        .child2 {
            color: blue;
        }
    "#;
    let stylesheet = parse_stylesheet(css).unwrap();
    let styles = compute_styles(&doc, &stylesheet);

    // Parent verification
    let parent_style = styles.get(parent).expect("Parent style missing");
    assert_eq!(parent_style.color, Color::RED);
    assert_eq!(parent_style.margin_top, LengthOrAuto::Px(10.0));
    assert_eq!(parent_style.margin_right, LengthOrAuto::Px(10.0));
    assert_eq!(parent_style.margin_bottom, LengthOrAuto::Px(10.0));
    assert_eq!(parent_style.margin_left, LengthOrAuto::Px(10.0));
    assert_eq!(parent_style.width, LengthOrAuto::Px(200.0));
    assert_eq!(parent_style.display, Display::Block);

    // Child1 verification: inherits color: red from parent; does NOT inherit margin or width!
    let child1_style = styles.get(child1).expect("Child1 style missing");
    assert_eq!(child1_style.color, Color::RED); // INHERITED!
    assert_eq!(child1_style.margin_top, LengthOrAuto::Px(0.0)); // NOT inherited
    assert_eq!(child1_style.width, LengthOrAuto::Auto); // NOT inherited
    assert_eq!(child1_style.display, Display::Inline);

    // Child2 verification: inherits color, but overrides with .child2 { color: blue; }
    let child2_style = styles.get(child2).expect("Child2 style missing");
    assert_eq!(child2_style.color, Color::BLUE); // OVERRIDDEN!
    assert_eq!(child2_style.margin_top, LengthOrAuto::Px(0.0));
}

#[test]
fn test_specificity_and_source_order_cascade() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut attrs = IndexMap::new();
    attrs.insert("class".to_string(), "box".to_string());
    let div = doc.create_element("div", attrs);
    doc.append_child(root, div).unwrap();

    // Specificity test: div.box (0,1,1) > .box (0,1,0) > div (0,0,1)
    let css1 = r#"
        div.box { color: blue; }
        .box { color: green; }
        div { color: red; }
    "#;
    let sheet1 = parse_stylesheet(css1).unwrap();
    let styles1 = compute_styles(&doc, &sheet1);
    assert_eq!(styles1.get(div).unwrap().color, Color::BLUE);

    // Source order test with same specificity: later rule wins
    let css2 = r#"
        .box { color: red; }
        .box { color: green; }
    "#;
    let sheet2 = parse_stylesheet(css2).unwrap();
    let styles2 = compute_styles(&doc, &sheet2);
    assert_eq!(styles2.get(div).unwrap().color, Color::GREEN);
}

#[test]
fn test_inline_style_attribute_overrides() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut attrs = IndexMap::new();
    attrs.insert("class".to_string(), "card".to_string());
    attrs.insert(
        "style".to_string(),
        "background-color: #ff5722; width: 200px; height: 100px; margin: 20px;".to_string(),
    );
    let div = doc.create_element("div", attrs);
    doc.append_child(root, div).unwrap();

    let css = r#"
        .card {
            background-color: #2b3a4a;
            width: 400px;
        }
    "#;
    let stylesheet = parse_stylesheet(css).unwrap();
    let styles = compute_styles(&doc, &stylesheet);
    let div_style = styles.get(div).unwrap();

    // Inline style overrides stylesheet rule
    assert_eq!(div_style.background_color, Color::rgb(255, 87, 34));
    assert_eq!(div_style.width, LengthOrAuto::Px(200.0));
    assert_eq!(div_style.height, LengthOrAuto::Px(100.0));
    assert_eq!(div_style.margin_top, LengthOrAuto::Px(20.0));
}

#[test]
fn test_inline_style_error_recovery_preserves_valid_declarations() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut attrs = IndexMap::new();
    attrs.insert(
        "style".to_string(),
        "color: ; width: 250px; invalid-prop; height: 80px".to_string(),
    );
    let div = doc.create_element("div", attrs);
    doc.append_child(root, div).unwrap();

    let stylesheet = parse_stylesheet("").unwrap();
    let styles = compute_styles(&doc, &stylesheet);
    let div_style = styles.get(div).unwrap();

    assert_eq!(div_style.width, LengthOrAuto::Px(250.0));
    assert_eq!(div_style.height, LengthOrAuto::Px(80.0));
}

#[test]
fn test_parse_real_world_css_example_com() {
    let css = r#"html{color-scheme:light dark;background:light-dark(#eee,#222)}body{font:16px/1.6 system-ui,sans-serif;max-width:26em;margin:auto;padding:25vh 2em 2em;text-align:center}"#;
    let stylesheet = parse_stylesheet(css).expect("parse example.com css");
    assert_eq!(stylesheet.rules.len(), 2);

    let r_html = &stylesheet.rules[0];
    assert_eq!(r_html.selectors[0].tag_name.as_deref(), Some("html"));
    assert!(r_html.declarations.iter().any(|d| d.name == "background"));

    let r_body = &stylesheet.rules[1];
    assert_eq!(r_body.selectors[0].tag_name.as_deref(), Some("body"));
    assert!(r_body.declarations.iter().any(|d| d.name == "margin"));
}

#[test]
fn test_parse_em_and_rem_units() {
    use crate::ast::{Unit, Value};

    let css = r#"
        .box {
            font-size: 2em;
            width: 26em;
            height: 1.5rem;
            margin: 0.5rem 1em;
            max-width: 30em;
            text-align: center;
        }
    "#;
    let stylesheet = parse_stylesheet(css).expect("parse em and rem css");
    assert_eq!(stylesheet.rules.len(), 1);
    let rule = &stylesheet.rules[0];

    let decl_map: std::collections::HashMap<&str, &Value> = rule
        .declarations
        .iter()
        .map(|d| (d.name.as_str(), &d.value))
        .collect();

    assert_eq!(
        decl_map.get("font-size").unwrap(),
        &&Value::Length(2.0, Unit::Em)
    );
    assert_eq!(
        decl_map.get("width").unwrap(),
        &&Value::Length(26.0, Unit::Em)
    );
    assert_eq!(
        decl_map.get("height").unwrap(),
        &&Value::Length(1.5, Unit::Rem)
    );
    assert_eq!(
        decl_map.get("max-width").unwrap(),
        &&Value::Length(30.0, Unit::Em)
    );
    assert_eq!(
        decl_map.get("text-align").unwrap(),
        &&Value::Keyword("center".to_string())
    );

    if let Value::List(items) = decl_map.get("margin").unwrap() {
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Value::Length(0.5, Unit::Rem));
        assert_eq!(items[1], Value::Length(1.0, Unit::Em));
    } else {
        panic!("Expected margin to be Value::List");
    }
}

#[test]
fn test_resolve_em_and_rem_to_concrete_pixels() {
    use crate::TextAlign;

    let mut doc = Document::new();
    let root = doc.root();

    let mut html_attrs = IndexMap::new();
    html_attrs.insert("style".to_string(), "font-size: 20px;".to_string());
    let html = doc.create_element("html", html_attrs);
    doc.append_child(root, html).unwrap();

    let mut parent_attrs = IndexMap::new();
    parent_attrs.insert("class".to_string(), "parent".to_string());
    let parent = doc.create_element("div", parent_attrs);
    doc.append_child(html, parent).unwrap();

    let mut child_attrs = IndexMap::new();
    child_attrs.insert("class".to_string(), "child".to_string());
    let child = doc.create_element("p", child_attrs);
    doc.append_child(parent, child).unwrap();

    let css = r#"
        .parent {
            font-size: 2rem; /* 2 * 20px = 40px */
            width: 10rem;    /* 10 * 20px = 200px */
            text-align: center;
        }
        .child {
            /* 1em for font-size refers to parent font-size (40px) */
            font-size: 0.5em; /* 0.5 * 40px = 20px */
            /* 1em for other properties refers to element's current font-size (20px) */
            width: 15em;     /* 15 * 20px = 300px */
            height: 2rem;    /* 2 * root (20px) = 40px */
            max-width: 20em; /* 20 * 20px = 400px */
            padding-top: 1em; /* 1 * 20px = 20px */
            margin-left: 2rem; /* 2 * 20px = 40px */
        }
    "#;
    let stylesheet = parse_stylesheet(css).unwrap();
    let styles = compute_styles(&doc, &stylesheet);

    let parent_style = styles.get(parent).expect("parent style");
    assert_eq!(parent_style.font_size, 40.0);
    assert_eq!(parent_style.width, LengthOrAuto::Px(200.0));
    assert_eq!(parent_style.text_align, TextAlign::Center);

    let child_style = styles.get(child).expect("child style");
    // font-size resolved against parent: 0.5 * 40px = 20px
    assert_eq!(child_style.font_size, 20.0);
    // width resolved against element's font-size (20px): 15 * 20px = 300px
    assert_eq!(child_style.width, LengthOrAuto::Px(300.0));
    // height resolved against root font-size (20px): 2 * 20px = 40px
    assert_eq!(child_style.height, LengthOrAuto::Px(40.0));
    // max-width resolved against element's font-size (20px): 20 * 20px = 400px
    assert_eq!(child_style.max_width, Some(400.0));
    // padding-top: 1em = 20px
    assert_eq!(child_style.padding_top, 20.0);
    // margin-left: 2rem = 40px
    assert_eq!(child_style.margin_left, LengthOrAuto::Px(40.0));
    // text-align inherited from parent
    assert_eq!(child_style.text_align, TextAlign::Center);
}

#[test]
fn test_flexbox_css_properties_parsing_and_computing() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut nav_attrs = IndexMap::new();
    nav_attrs.insert("class".to_string(), "navbar".to_string());
    let nav = doc.create_element("nav", nav_attrs);
    doc.append_child(root, nav).unwrap();

    let mut item_attrs = IndexMap::new();
    item_attrs.insert("class".to_string(), "item".to_string());
    let item = doc.create_element("div", item_attrs);
    doc.append_child(nav, item).unwrap();

    let css = r#"
        .navbar {
            display: flex;
            flex-direction: row;
            justify-content: space-between;
            align-items: center;
            gap: 1.5rem;
        }
        .item {
            flex-grow: 2.5;
            gap: 10px;
        }
        .col-container {
            display: flex;
            flex-direction: column;
            justify-content: center;
            align-items: flex-end;
            gap: 2em;
        }
    "#;
    let stylesheet = parse_stylesheet(css).unwrap();
    let styles = compute_styles(&doc, &stylesheet);

    let nav_style = styles.get(nav).expect("nav style");
    assert_eq!(nav_style.display, Display::Flex);
    assert_eq!(nav_style.flex_direction, FlexDirection::Row);
    assert_eq!(nav_style.justify_content, JustifyContent::SpaceBetween);
    assert_eq!(nav_style.align_items, AlignItems::Center);
    // root font size is default 16px -> 1.5rem = 24px
    assert_eq!(nav_style.gap, 24.0);
    assert_eq!(nav_style.flex_grow, 0.0);

    let item_style = styles.get(item).expect("item style");
    assert_eq!(item_style.flex_grow, 2.5);
    assert_eq!(item_style.gap, 10.0);
}

#[test]
fn test_at_rules_descendant_selectors_important_and_borders() {
    let mut doc = Document::new();
    let root = doc.root();

    let mut container_attrs = IndexMap::new();
    container_attrs.insert("class".to_string(), "container".to_string());
    let container = doc.create_element("div", container_attrs);
    doc.append_child(root, container).unwrap();

    let p = doc.create_element("p", IndexMap::new());
    doc.append_child(container, p).unwrap();

    let mut a_attrs = IndexMap::new();
    a_attrs.insert("href".to_string(), "https://example.com".to_string());
    let a = doc.create_element("a", a_attrs);
    doc.append_child(p, a).unwrap();

    let css = r#"
        @charset "utf-8";
        @import "reset.css";

        .container {
            border: 2px solid #ff0000;
            color: green !important;
        }
        #override-attempt.container {
            color: yellow;
        }
        div p {
            border-bottom: 4px solid blue;
        }
        p > a, a:visited, a:hover {
            color: rgb(10, 20, 30);
        }
    "#;

    let stylesheet = parse_stylesheet(css).expect("parse stylesheet");
    // All 4 style rules after @charset and @import must be preserved
    assert_eq!(stylesheet.rules.len(), 4);

    let styles = compute_styles(&doc, &stylesheet);

    let container_style = styles.get(container).expect("container style");
    assert_eq!(container_style.border_top, 2.0);
    assert_eq!(container_style.border_right, 2.0);
    assert_eq!(container_style.border_bottom, 2.0);
    assert_eq!(container_style.border_left, 2.0);
    assert_eq!(container_style.border_color, Some(Color::RED));
    assert_eq!(container_style.color, Color::GREEN);

    let p_style = styles.get(p).expect("p style");
    assert_eq!(p_style.border_bottom, 4.0);
    assert_eq!(p_style.border_top, 0.0);
    assert_eq!(p_style.border_color, Some(Color::BLUE));

    let a_style = styles.get(a).expect("a style");
    assert_eq!(a_style.color, Color::rgb(10, 20, 30));
}
