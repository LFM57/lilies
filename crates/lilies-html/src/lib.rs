#![deny(unsafe_code)]

pub mod error;
pub mod parser;
pub mod style;
pub mod token;
pub mod tokenizer;

pub use error::HtmlError;
pub use parser::{is_void_element, parse_html};
pub use style::{
    compute_styles_for_document, extract_style_text, extract_styles, parse_styles_from_document,
};
pub use token::Token;
pub use tokenizer::{Tokenizer, TokenizerState};

#[cfg(test)]
mod tests;
