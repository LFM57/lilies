#![deny(unsafe_code)]

use crate::ast::{Declaration, StyleSheet, Unit, Value};
use crate::color::Color;
use crate::matcher::matches;
use crate::specificity::Specificity;
use lilies_dom::{Document, NodeId};
use std::collections::HashMap;

/// CSS `display` property values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Inline,
    Block,
    None,
}

/// A CSS length or the `auto` keyword.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LengthOrAuto {
    #[default]
    Auto,
    Px(f32),
}

impl LengthOrAuto {
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    pub fn to_px(&self) -> Option<f32> {
        match self {
            Self::Px(px) => Some(*px),
            Self::Auto => None,
        }
    }
}

/// Computed styles for a DOM element after cascade and inheritance.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub width: LengthOrAuto,
    pub height: LengthOrAuto,
    pub margin_top: LengthOrAuto,
    pub margin_right: LengthOrAuto,
    pub margin_bottom: LengthOrAuto,
    pub margin_left: LengthOrAuto,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub background_color: Color,
    pub color: Color,
    pub font_size: f32,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            width: LengthOrAuto::Auto,
            height: LengthOrAuto::Auto,
            margin_top: LengthOrAuto::Px(0.0),
            margin_right: LengthOrAuto::Px(0.0),
            margin_bottom: LengthOrAuto::Px(0.0),
            margin_left: LengthOrAuto::Px(0.0),
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            background_color: Color::TRANSPARENT,
            color: Color::BLACK,
            font_size: 16.0,
        }
    }
}

impl ComputedStyle {
    /// Returns default initial style based on the HTML element tag.
    pub fn default_for_tag(tag_name: &str) -> Self {
        let tag = tag_name.to_ascii_lowercase();
        let display = match tag.as_str() {
            "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul"
            | "ol" | "li" | "section" | "header" | "footer" | "article" | "nav" | "main"
            | "blockquote" | "form" | "hr" | "table" => Display::Block,
            "head" | "style" | "script" | "title" | "meta" | "link" => Display::None,
            _ => Display::Inline,
        };

        let font_size = match tag.as_str() {
            "h1" => 32.0,
            "h2" => 24.0,
            "h3" => 20.0,
            "h4" => 16.0,
            "h5" => 14.0,
            "h6" => 12.0,
            _ => 16.0,
        };

        Self {
            display,
            font_size,
            ..Default::default()
        }
    }

    /// Updates this computed style with a matched CSS property declaration.
    pub fn apply_declaration(&mut self, declaration: &Declaration) {
        let prop = declaration.name.to_ascii_lowercase();
        let val = &declaration.value;

        match prop.as_str() {
            "display" => {
                if let Some(k) = val.as_keyword() {
                    match k.to_ascii_lowercase().as_str() {
                        "block" => self.display = Display::Block,
                        "inline" => self.display = Display::Inline,
                        "none" => self.display = Display::None,
                        _ => {}
                    }
                }
            }

            "width" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.width = l;
                }
            }

            "height" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.height = l;
                }
            }

            "color" => {
                if let Some(c) = val.to_color() {
                    self.color = c;
                }
            }

            "font-size" => {
                if let Some(p) = parse_length(val) {
                    self.font_size = p;
                }
            }

            "background-color" | "background" => {
                if let Some(c) = val.to_color() {
                    self.background_color = c;
                }
            }

            "margin-top" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.margin_top = l;
                }
            }
            "margin-right" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.margin_right = l;
                }
            }
            "margin-bottom" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.margin_bottom = l;
                }
            }
            "margin-left" => {
                if let Some(l) = parse_length_or_auto(val) {
                    self.margin_left = l;
                }
            }

            "margin" => {
                self.apply_margin_shorthand(val);
            }

            "padding-top" => {
                if let Some(p) = parse_length(val) {
                    self.padding_top = p;
                }
            }
            "padding-right" => {
                if let Some(p) = parse_length(val) {
                    self.padding_right = p;
                }
            }
            "padding-bottom" => {
                if let Some(p) = parse_length(val) {
                    self.padding_bottom = p;
                }
            }
            "padding-left" => {
                if let Some(p) = parse_length(val) {
                    self.padding_left = p;
                }
            }

            "padding" => {
                self.apply_padding_shorthand(val);
            }

            _ => {}
        }
    }

    fn apply_margin_shorthand(&mut self, val: &Value) {
        match val {
            Value::List(items) => {
                let parsed: Vec<Option<LengthOrAuto>> =
                    items.iter().map(parse_length_or_auto).collect();
                match parsed.as_slice() {
                    [Some(top_bottom), Some(right_left)] => {
                        self.margin_top = *top_bottom;
                        self.margin_bottom = *top_bottom;
                        self.margin_right = *right_left;
                        self.margin_left = *right_left;
                    }
                    [Some(top), Some(right_left), Some(bottom)] => {
                        self.margin_top = *top;
                        self.margin_right = *right_left;
                        self.margin_left = *right_left;
                        self.margin_bottom = *bottom;
                    }
                    [Some(top), Some(right), Some(bottom), Some(left)] => {
                        self.margin_top = *top;
                        self.margin_right = *right;
                        self.margin_bottom = *bottom;
                        self.margin_left = *left;
                    }
                    _ => {}
                }
            }
            single => {
                if let Some(l) = parse_length_or_auto(single) {
                    self.margin_top = l;
                    self.margin_right = l;
                    self.margin_bottom = l;
                    self.margin_left = l;
                }
            }
        }
    }

    fn apply_padding_shorthand(&mut self, val: &Value) {
        match val {
            Value::List(items) => {
                let parsed: Vec<Option<f32>> = items.iter().map(parse_length).collect();
                match parsed.as_slice() {
                    [Some(top_bottom), Some(right_left)] => {
                        self.padding_top = *top_bottom;
                        self.padding_bottom = *top_bottom;
                        self.padding_right = *right_left;
                        self.padding_left = *right_left;
                    }
                    [Some(top), Some(right_left), Some(bottom)] => {
                        self.padding_top = *top;
                        self.padding_right = *right_left;
                        self.padding_left = *right_left;
                        self.padding_bottom = *bottom;
                    }
                    [Some(top), Some(right), Some(bottom), Some(left)] => {
                        self.padding_top = *top;
                        self.padding_right = *right;
                        self.padding_bottom = *bottom;
                        self.padding_left = *left;
                    }
                    _ => {}
                }
            }
            single => {
                if let Some(p) = parse_length(single) {
                    self.padding_top = p;
                    self.padding_right = p;
                    self.padding_bottom = p;
                    self.padding_left = p;
                }
            }
        }
    }
}

fn parse_length_or_auto(val: &Value) -> Option<LengthOrAuto> {
    match val {
        Value::Keyword(k) if k.eq_ignore_ascii_case("auto") => Some(LengthOrAuto::Auto),
        Value::Keyword(k) if k == "0" => Some(LengthOrAuto::Px(0.0)),
        Value::Length(px, Unit::Px) => Some(LengthOrAuto::Px(*px)),
        _ => None,
    }
}

fn parse_length(val: &Value) -> Option<f32> {
    match val {
        Value::Length(px, Unit::Px) => Some(*px),
        Value::Keyword(k) if k == "0" => Some(0.0),
        _ => None,
    }
}

/// Storage container for all computed styles across the document.
#[derive(Debug, Clone, Default)]
pub struct StyleTree {
    pub styles: HashMap<NodeId, ComputedStyle>,
}

impl StyleTree {
    pub fn new() -> Self {
        Self {
            styles: HashMap::new(),
        }
    }

    pub fn insert(&mut self, node_id: NodeId, style: ComputedStyle) {
        self.styles.insert(node_id, style);
    }

    pub fn get(&self, node_id: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(&node_id)
    }

    pub fn get_mut(&mut self, node_id: NodeId) -> Option<&mut ComputedStyle> {
        self.styles.get_mut(&node_id)
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &ComputedStyle)> {
        self.styles.iter()
    }

    pub fn into_inner(self) -> HashMap<NodeId, ComputedStyle> {
        self.styles
    }
}

/// Cascade Engine: matches rules, resolves specificity and order,
/// and applies inheritance down the DOM tree.
pub fn compute_styles(doc: &Document, stylesheet: &StyleSheet) -> StyleTree {
    let mut style_tree = StyleTree::new();
    let root = doc.root();
    compute_node(root, None, doc, stylesheet, &mut style_tree);
    style_tree
}

fn compute_node(
    node_id: NodeId,
    parent_style: Option<&ComputedStyle>,
    doc: &Document,
    stylesheet: &StyleSheet,
    style_tree: &mut StyleTree,
) {
    let Some(node) = doc.node(node_id) else {
        return;
    };

    if let Some(element) = node.as_element() {
        let mut style = ComputedStyle::default_for_tag(element.tag_name());
        let tag = element.tag_name().to_ascii_lowercase();
        let is_heading = matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");

        // Apply inheritable properties from parent
        if let Some(parent) = parent_style {
            style.color = parent.color;
            if !is_heading {
                style.font_size = parent.font_size;
            }
        }

        // Find all matching rules and collect declarations with their specificity and rule order
        let mut matched: Vec<(Specificity, usize, &Declaration)> = Vec::new();
        for (rule_idx, rule) in stylesheet.rules.iter().enumerate() {
            let best_spec = rule
                .selectors
                .iter()
                .filter(|sel| matches(sel, node_id, doc.arena()))
                .map(|sel| sel.specificity())
                .max();

            if let Some(spec) = best_spec {
                for decl in &rule.declarations {
                    matched.push((spec, rule_idx, decl));
                }
            }
        }

        // Sort by specificity ascending, then by rule index (document order) ascending
        matched.sort_by(|(s1, r1, _), (s2, r2, _)| s1.cmp(s2).then_with(|| r1.cmp(r2)));

        // Apply matched declarations in cascading order
        for (_, _, decl) in matched {
            style.apply_declaration(decl);
        }

        // Apply inline style attribute with highest priority
        if let Some(style_attr) = element.get_attribute("style") {
            if let Ok(inline_decls) = crate::parser::parse_declarations(style_attr) {
                for decl in &inline_decls {
                    style.apply_declaration(decl);
                }
            }
        }

        style_tree.insert(node_id, style.clone());

        // Recurse for all children
        for child_id in doc.get_children(node_id) {
            compute_node(child_id, Some(&style), doc, stylesheet, style_tree);
        }
    } else {
        // Document node, text node, comment, or doctype
        if node.as_text().is_some() {
            let text_style = ComputedStyle {
                display: Display::Inline,
                font_size: parent_style.map_or(16.0, |p| p.font_size),
                color: parent_style.map_or(Color::BLACK, |p| p.color),
                ..Default::default()
            };
            style_tree.insert(node_id, text_style);
        }

        for child_id in doc.get_children(node_id) {
            compute_node(child_id, parent_style, doc, stylesheet, style_tree);
        }
    }
}
