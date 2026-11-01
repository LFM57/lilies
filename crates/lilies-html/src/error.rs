#![deny(unsafe_code)]

use lilies_dom::DomError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HtmlError {
    #[error("Tokenizer parse error: {0}")]
    TokenizerError(String),

    #[error("DOM manipulation error: {0}")]
    DomError(#[from] DomError),
}
