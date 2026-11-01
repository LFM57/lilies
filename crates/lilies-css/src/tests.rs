#![deny(unsafe_code)]

use crate::{compute_styles, matches, parse_stylesheet, Color, Display, LengthOrAuto, Specificity};
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
