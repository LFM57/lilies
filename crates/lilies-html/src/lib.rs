#![deny(unsafe_code)]

pub mod error;
pub mod parser;
pub mod token;
pub mod tokenizer;

pub use error::HtmlError;
pub use parser::{is_void_element, parse_html};
pub use token::Token;
pub use tokenizer::{Tokenizer, TokenizerState};

#[cfg(test)]
mod tests;
