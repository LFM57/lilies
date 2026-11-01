#![deny(unsafe_code)]

use crate::color::Color;

/// Supported CSS length units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Px,
    Em,
    Rem,
}

/// A parsed CSS value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Keyword(String),
    Length(f32, Unit),
    Color(Color),
    List(Vec<Value>),
}

impl Value {
    /// Attempts to convert this value into a `Color`.
    pub fn to_color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            Value::Keyword(k) => Color::from_name(k),
            _ => None,
        }
    }

    /// Attempts to convert this value into a pixel length.
    pub fn to_length(&self) -> Option<f32> {
        match self {
            Value::Length(px, Unit::Px) => Some(*px),
            _ => None,
        }
    }

    /// Converts this value into a pixel length using the provided element font size and root font size.
    pub fn to_length_with_context(&self, font_size: f32, root_font_size: f32) -> Option<f32> {
        match self {
            Value::Length(px, Unit::Px) => Some(*px),
            Value::Length(em, Unit::Em) => Some(*em * font_size),
            Value::Length(rem, Unit::Rem) => Some(*rem * root_font_size),
            Value::Keyword(k) if k == "0" => Some(0.0),
            _ => None,
        }
    }

    /// Returns the keyword string if this is a `Value::Keyword`.
    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Value::Keyword(k) => Some(k.as_str()),
            _ => None,
        }
    }

    /// Checks if this value is the keyword "auto".
    pub fn is_auto(&self) -> bool {
        matches!(self, Value::Keyword(k) if k.eq_ignore_ascii_case("auto"))
    }
}

/// A single CSS property declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: Value,
}

impl Declaration {
    pub fn new(name: impl Into<String>, value: Value) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// A CSS selector representing type, id, and class constraints on an element.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selector {
    pub tag_name: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

impl Selector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag_name = Some(tag.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        self.classes.push(class.into());
        self
    }
}

/// A CSS rule consisting of selectors and declarations.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

impl Rule {
    pub fn new(selectors: Vec<Selector>, declarations: Vec<Declaration>) -> Self {
        Self {
            selectors,
            declarations,
        }
    }
}

/// An entire parsed CSS stylesheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
}

impl StyleSheet {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    pub fn append(&mut self, mut other: StyleSheet) {
        self.rules.append(&mut other.rules);
    }

    pub fn merge(&mut self, other: StyleSheet) {
        self.rules.extend(other.rules);
    }
}

/// Alias for `StyleSheet` for casing flexibility.
pub type Stylesheet = StyleSheet;
