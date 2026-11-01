#![deny(unsafe_code)]

use crate::{Document, DomError};
use indexmap::IndexMap;

#[test]
fn test_append_child_and_traversal() {
    let mut doc = Document::new();
    let root = doc.root();

    let html = doc.create_element("html", IndexMap::new());
    let body = doc.create_element("body", IndexMap::new());
    let div = doc.create_element("div", IndexMap::new());
    let span = doc.create_element("span", IndexMap::new());

    doc.append_child(root, html).unwrap();
    doc.append_child(html, body).unwrap();
    doc.append_child(body, div).unwrap();
    doc.append_child(body, span).unwrap();

    // Check parent relationships
    assert_eq!(doc.parent(html), Some(root));
    assert_eq!(doc.parent(body), Some(html));
    assert_eq!(doc.parent(div), Some(body));
    assert_eq!(doc.parent(span), Some(body));

    // Check sibling relationships
    assert_eq!(doc.first_child(body), Some(div));
    assert_eq!(doc.last_child(body), Some(span));
    assert_eq!(doc.next_sibling(div), Some(span));
    assert_eq!(doc.prev_sibling(span), Some(div));
    assert_eq!(doc.prev_sibling(div), None);
    assert_eq!(doc.next_sibling(span), None);

    // Check get_children
    assert_eq!(doc.get_children(body), vec![div, span]);
}

#[test]
fn test_insert_before() {
    let mut doc = Document::new();
    let root = doc.root();

    let div1 = doc.create_element("div1", IndexMap::new());
    let div2 = doc.create_element("div2", IndexMap::new());
    let div3 = doc.create_element("div3", IndexMap::new());

    doc.append_child(root, div1).unwrap();
    doc.append_child(root, div3).unwrap();

    // Insert div2 before div3
    doc.insert_before(root, div2, div3).unwrap();

    assert_eq!(doc.get_children(root), vec![div1, div2, div3]);
    assert_eq!(doc.next_sibling(div1), Some(div2));
    assert_eq!(doc.next_sibling(div2), Some(div3));
    assert_eq!(doc.prev_sibling(div3), Some(div2));
    assert_eq!(doc.prev_sibling(div2), Some(div1));
}

#[test]
fn test_remove_child() {
    let mut doc = Document::new();
    let root = doc.root();

    let a = doc.create_element("a", IndexMap::new());
    let b = doc.create_element("b", IndexMap::new());
    let c = doc.create_element("c", IndexMap::new());

    doc.append_child(root, a).unwrap();
    doc.append_child(root, b).unwrap();
    doc.append_child(root, c).unwrap();

    // Remove middle child 'b'
    doc.remove_child(root, b).unwrap();

    assert_eq!(doc.get_children(root), vec![a, c]);
    assert_eq!(doc.next_sibling(a), Some(c));
    assert_eq!(doc.prev_sibling(c), Some(a));
    assert_eq!(doc.parent(b), None);
    assert_eq!(doc.prev_sibling(b), None);
    assert_eq!(doc.next_sibling(b), None);
}

#[test]
fn test_reparenting() {
    let mut doc = Document::new();
    let root = doc.root();

    let container1 = doc.create_element("div", IndexMap::new());
    let container2 = doc.create_element("div", IndexMap::new());
    let item = doc.create_element("span", IndexMap::new());

    doc.append_child(root, container1).unwrap();
    doc.append_child(root, container2).unwrap();
    doc.append_child(container1, item).unwrap();

    assert_eq!(doc.get_children(container1), vec![item]);
    assert_eq!(doc.get_children(container2), vec![]);

    // Move item to container2
    doc.append_child(container2, item).unwrap();

    assert_eq!(doc.get_children(container1), vec![]);
    assert_eq!(doc.get_children(container2), vec![item]);
    assert_eq!(doc.parent(item), Some(container2));
}

#[test]
fn test_hierarchy_cycle_prevention() {
    let mut doc = Document::new();
    let root = doc.root();

    let parent = doc.create_element("div", IndexMap::new());
    let child = doc.create_element("span", IndexMap::new());

    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();

    // Trying to append parent to child should fail with HierarchyRequestError
    let err = doc.append_child(child, parent).unwrap_err();
    assert_eq!(err, DomError::HierarchyRequestError);

    // Self append
    let err_self = doc.append_child(parent, parent).unwrap_err();
    assert_eq!(err_self, DomError::HierarchyRequestError);
}

#[test]
fn test_debug_printer() {
    let mut doc = Document::new();

    let mut attrs = IndexMap::new();
    attrs.insert("id".to_string(), "app".to_string());
    let div = doc.create_element("div", attrs);
    let text = doc.create_text("hello");

    doc.append_child(div, text).unwrap();

    let dumped = doc.dump_tree(div);
    let expected = "<div id=\"app\">\n  #text \"hello\"";
    assert_eq!(dumped, expected);
}

#[test]
fn test_element_attributes_and_classes() {
    let mut doc = Document::new();
    let mut attrs = IndexMap::new();
    attrs.insert("id".to_string(), "main".to_string());
    attrs.insert("class".to_string(), "btn btn-primary active".to_string());

    let el_id = doc.create_element("button", attrs);
    let node = doc.node(el_id).unwrap();
    let el = node.as_element().unwrap();

    assert_eq!(el.id(), Some("main"));
    assert!(el.has_class("btn"));
    assert!(el.has_class("btn-primary"));
    assert!(el.has_class("active"));
    assert!(!el.has_class("disabled"));
    assert_eq!(el.classes(), vec!["btn", "btn-primary", "active"]);
}
