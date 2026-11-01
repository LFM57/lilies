#![deny(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LayoutError {
    #[error("Layout error: {0}")]
    Generic(String),
}
