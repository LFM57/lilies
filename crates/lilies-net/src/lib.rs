#![deny(unsafe_code)]

pub mod error;
pub mod fetch;

pub use error::NetError;
pub use fetch::{fetch, MAX_REDIRECTS, USER_AGENT};

#[cfg(test)]
mod tests;
