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

/// Represents a source of CSS styles in an HTML document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleSource {
    /// An inline `<style>` tag containing raw CSS text.
    StyleTag(String),
    /// An external `<link rel="stylesheet" href="...">` reference.
    LinkHref(String),
}

/// Resolves a potentially relative `href` against the `base_url`.
pub fn resolve_url(base_url: &str, href: &str) -> String {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return base_url.to_string();
    }

    // Strip optional "(offline demo)" suffix before resolving
    let clean_base = base_url
        .split_once(" (offline demo)")
        .map_or(base_url, |(b, _)| b);

    if let Ok(base) = url::Url::parse(clean_base) {
        if let Ok(resolved) = base.join(trimmed) {
            return resolved.to_string();
        }
    }

    // Fallback if href has explicit scheme
    if trimmed.contains("://") || trimmed.starts_with("file:") {
        return trimmed.to_string();
    }

    // Fallback path joining
    if trimmed.starts_with('/') {
        if let Some((scheme, rest)) = clean_base.split_once("://") {
            let host = rest.split('/').next().unwrap_or(rest);
            return format!("{}://{}{}", scheme, host, trimmed);
        }
    }

    format!(
        "{}/{}",
        clean_base.trim_end_matches('/'),
        trimmed.trim_start_matches('/')
    )
}

/// Traverses the document tree to find all `<link rel="stylesheet">` elements
/// that have an `href` attribute, returning their `href` strings in document order.
pub fn find_stylesheet_links(doc: &Document) -> Vec<String> {
    extract_style_sources(doc)
        .into_iter()
        .filter_map(|s| match s {
            StyleSource::LinkHref(href) => Some(href),
            _ => None,
        })
        .collect()
}

/// Extracts all style sources (both `<style>` blocks and `<link rel="stylesheet">` hrefs)
/// from the document in document traversal order.
pub fn extract_style_sources(doc: &Document) -> Vec<StyleSource> {
    let mut out = Vec::new();
    collect_style_sources(doc, doc.root(), &mut out);
    out
}

fn collect_style_sources(doc: &Document, node_id: NodeId, out: &mut Vec<StyleSource>) {
    if let Some(node) = doc.node(node_id) {
        if let Some(el) = node.as_element() {
            let tag = el.tag_name();
            if tag.eq_ignore_ascii_case("style") {
                let mut style_content = String::new();
                collect_text_nodes(doc, node_id, &mut style_content);
                if !style_content.trim().is_empty() {
                    out.push(StyleSource::StyleTag(style_content));
                }
                return;
            } else if tag.eq_ignore_ascii_case("link") {
                let is_stylesheet = el.get_attribute("rel").is_some_and(|r| {
                    r.split_whitespace()
                        .any(|tok| tok.eq_ignore_ascii_case("stylesheet"))
                });
                if is_stylesheet {
                    if let Some(href) = el.get_attribute("href") {
                        let trimmed = href.trim();
                        if !trimmed.is_empty() {
                            out.push(StyleSource::LinkHref(trimmed.to_string()));
                        }
                    }
                }
            }
        }
    }

    for child in doc.get_children(node_id) {
        collect_style_sources(doc, child, out);
    }
}

/// Fetches and merges all stylesheet sources (both `<link rel="stylesheet">` and `<style>`)
/// using `lilies_net::fetch` into a single `StyleSheet`.
pub fn fetch_styles_from_document(doc: &Document, base_url: &str) -> StyleSheet {
    fetch_styles_with_fetcher(doc, base_url, |url| {
        lilies_net::fetch(url).map_err(|e| e.to_string())
    })
}

/// Fetches and merges all stylesheet sources using a custom fetcher closure into a `StyleSheet`.
pub fn fetch_styles_with_fetcher<F, E>(doc: &Document, base_url: &str, mut fetcher: F) -> StyleSheet
where
    F: FnMut(&str) -> Result<String, E>,
    E: std::fmt::Display,
{
    let mut merged = StyleSheet::new();
    for source in extract_style_sources(doc) {
        match source {
            StyleSource::StyleTag(css) => {
                if let Ok(sheet) = parse_stylesheet(&css) {
                    merged.append(sheet);
                }
            }
            StyleSource::LinkHref(href) => {
                let resolved = resolve_url(base_url, &href);
                match fetcher(&resolved) {
                    Ok(css) => {
                        if let Ok(sheet) = parse_stylesheet(&css) {
                            merged.append(sheet);
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "Failed to fetch external stylesheet '{}' (resolved '{}'): {}",
                            href, resolved, err
                        );
                    }
                }
            }
        }
    }
    merged
}

/// Loads and computes styles for `doc`, fetching any external `<link rel="stylesheet">`
/// resources over HTTP/file using `lilies_net::fetch` and merging them with embedded `<style>` tags.
pub fn load_styles_with_external(doc: &Document, base_url: &str) -> StyleTree {
    let stylesheet = fetch_styles_from_document(doc, base_url);
    compute_styles(doc, &stylesheet)
}

/// Loads and computes styles for `doc` using a custom fetcher closure (e.g. for testing).
pub fn load_styles_with_fetcher<F, E>(doc: &Document, base_url: &str, fetcher: F) -> StyleTree
where
    F: FnMut(&str) -> Result<String, E>,
    E: std::fmt::Display,
{
    let stylesheet = fetch_styles_with_fetcher(doc, base_url, fetcher);
    compute_styles(doc, &stylesheet)
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
