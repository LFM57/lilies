#![deny(unsafe_code)]

pub mod ast;
pub mod color;
pub mod computed;
pub mod error;
pub mod matcher;
pub mod parser;
pub mod specificity;

pub use ast::{Declaration, Rule, Selector, StyleSheet, Stylesheet, Unit, Value};
pub use color::Color;
pub use computed::{
    compute_styles, compute_styles_for_document, parse_styles_from_document, ComputedStyle,
    Display, LengthOrAuto, StyleTree,
};
pub use error::CssError;
pub use matcher::{matches, matches_document};
pub use parser::{parse_declarations, parse_stylesheet, CssParser};
pub use specificity::Specificity;

#[cfg(test)]
mod tests;
