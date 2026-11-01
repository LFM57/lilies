#![deny(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum CssError {
    #[error("CSS Parse error: {0}")]
    ParseError(String),

    #[error("Unexpected end of file")]
    UnexpectedEof,
}
