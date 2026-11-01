#![deny(unsafe_code)]

pub mod builder;
pub mod error;
pub mod parser;
pub mod style;
pub mod token;
pub mod tokenizer;

pub use error::HtmlError;
pub use parser::{is_void_element, parse_html};
pub use style::{
    compute_styles_for_document, extract_style_sources, extract_style_text, extract_styles,
    fetch_styles_from_document, fetch_styles_with_fetcher, find_stylesheet_links,
    load_styles_with_external, load_styles_with_fetcher, parse_styles_from_document, resolve_url,
    StyleSource,
};
pub use token::Token;
pub use tokenizer::{Tokenizer, TokenizerState};

#[cfg(test)]
mod tests;
