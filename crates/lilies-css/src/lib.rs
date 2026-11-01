#![deny(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CssError {
    #[error("CSS Parse error: {0}")]
    ParseError(String),
}

/// A CSS declaration consisting of a property name and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
}

/// A CSS rule consisting of selectors and declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<String>,
    pub declarations: Vec<Declaration>,
}

/// A parsed CSS StyleSheet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
}

impl StyleSheet {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }
}

/// Placeholder CSS parser for Milestone 1.
pub fn parse_stylesheet(_css: &str) -> Result<StyleSheet, CssError> {
    Ok(StyleSheet::new())
}
