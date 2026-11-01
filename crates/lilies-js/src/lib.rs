#![deny(unsafe_code)]

pub mod error;
pub mod runtime;

pub use error::JsError;
pub use runtime::{camel_to_kebab, find_element_by_id, ElementNode, JsRuntime};

#[cfg(test)]
mod tests;
