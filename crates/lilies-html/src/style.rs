#![deny(unsafe_code)]

use lilies_css::{compute_styles, parse_stylesheet, StyleSheet, StyleTree};
use lilies_dom::{Document, NodeId};

/// Traverses the document tree to find all `<style>` elements and extracts their text content,
/// concatenated into a single CSS string.
pub fn extract_style_text(doc: &Document) -> String {
    let mut out = String::new();
    collect_style_nodes(doc, doc.root(), &mut out);
    out
}

/// Traverses the document tree to find all `<style>` elements and returns each `<style>` block's content.
pub fn extract_styles(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    collect_style_blocks(doc, doc.root(), &mut out);
    out
}

/// Extracts all `<style>` elements from the document, concatenates their text,
/// and parses them into a `lilies_css::StyleSheet`.
pub fn parse_styles_from_document(doc: &Document) -> StyleSheet {
    let css = extract_style_text(doc);
    parse_stylesheet(&css).unwrap_or_default()
}

/// Helper that extracts `<style>` blocks from `doc`, parses them into a `StyleSheet`,
/// and computes the `StyleTree` cascading rules from `<style>` tags and inline `style="..."` attributes.
pub fn compute_styles_for_document(doc: &Document) -> StyleTree {
    let stylesheet = parse_styles_from_document(doc);
    compute_styles(doc, &stylesheet)
}

fn collect_style_nodes(doc: &Document, node_id: NodeId, out: &mut String) {
    if let Some(node) = doc.node(node_id) {
        if let Some(el) = node.as_element() {
            if el.tag_name().eq_ignore_ascii_case("style") {
                let mut style_content = String::new();
                collect_text_nodes(doc, node_id, &mut style_content);
                if !style_content.is_empty() {
                    out.push_str(&style_content);
                    out.push('\n');
                }
                return;
            }
        }
    }

    for child in doc.get_children(node_id) {
        collect_style_nodes(doc, child, out);
    }
}

fn collect_style_blocks(doc: &Document, node_id: NodeId, out: &mut Vec<String>) {
    if let Some(node) = doc.node(node_id) {
        if let Some(el) = node.as_element() {
            if el.tag_name().eq_ignore_ascii_case("style") {
                let mut style_content = String::new();
                collect_text_nodes(doc, node_id, &mut style_content);
                out.push(style_content);
                return;
            }
        }
    }

    for child in doc.get_children(node_id) {
        collect_style_blocks(doc, child, out);
    }
}

fn collect_text_nodes(doc: &Document, node_id: NodeId, out: &mut String) {
    for child in doc.get_children(node_id) {
        if let Some(node) = doc.node(child) {
            if let Some(text) = node.as_text() {
                out.push_str(text.data());
            }
        }
        collect_text_nodes(doc, child, out);
    }
}
