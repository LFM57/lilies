//! JavaScript runtime and DOM bindings for the lilies browser engine.
//!
//! Powered by [`boa_engine`], `lilies-js` provides:
//! - Full DOM tree querying (`querySelector`, `querySelectorAll`, `getElementById`, `getElementsByClassName`, `getElementsByName`, `getElementsByTagName`, `matches`, `closest`).
//! - Live DOM manipulation and serialization (`innerHTML`, `outerHTML`, `createElement`, `appendChild`, `insertBefore`, `removeChild`, `remove`, `replaceChild`, `cloneNode`).
//! - Traversal and element collections (`children`, `childNodes`, `firstChild`, `lastChild`, `firstElementChild`, `lastElementChild`, `nextSibling`, `previousSibling`, `nextElementSibling`, `previousElementSibling`).
//! - Standard element properties (`classList`, `style` declaration proxy, `id`, `className`, `value`, `checked`, `disabled`, `src`, `href`, `nodeType`, `nodeName`, `nodeValue`).
//! - W3C event system with bubbling, `stopPropagation()`, `preventDefault()`, `addEventListener`, `removeEventListener`, `dispatchEvent`, `Event`, and `CustomEvent`.
//! - Asynchronous timer and animation scheduling (`setTimeout`, `setInterval`, `requestAnimationFrame`).
//! - Global Web APIs (`window.performance.now()`, `localStorage`, `sessionStorage`, `history`, `URL`, `URLSearchParams`, `btoa`, `atob`, `fetch`).

#![deny(unsafe_code)]

pub mod error;
pub mod runtime;

pub use error::JsError;
pub use runtime::{camel_to_kebab, find_element_by_id, ElementNode, JsRuntime};

#[cfg(test)]
mod tests;
