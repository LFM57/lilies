#![deny(unsafe_code)]

pub mod arena;
pub mod document;
pub mod error;
pub mod node;

pub use arena::{Arena, NodeId};
pub use document::Document;
pub use error::DomError;
pub use node::{DocumentType, Element, Node, NodeType, Text};

#[cfg(test)]
mod tests;
