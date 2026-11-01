#![deny(unsafe_code)]

use crate::ast::Selector;
use lilies_dom::{Arena, Document, NodeId};

/// Determines whether a DOM node matches the given selector.
///
/// Returns `false` if the node is not an Element or if any selector constraint fails.
pub fn matches(selector: &Selector, node_id: NodeId, dom: &Arena) -> bool {
    let Some(node) = dom.get(node_id) else {
        return false;
    };
    let Some(element) = node.as_element() else {
        return false;
    };

    // Check tag name
    if let Some(ref tag) = selector.tag_name {
        if !element.tag_name().eq_ignore_ascii_case(tag) {
            return false;
        }
    }

    // Check id
    if let Some(ref id) = selector.id {
        if element.id() != Some(id.as_str()) {
            return false;
        }
    }

    // Check all classes
    for class in &selector.classes {
        if !element.has_class(class) {
            return false;
        }
    }

    true
}

/// Convenience helper to match against a `Document`.
pub fn matches_document(selector: &Selector, node_id: NodeId, doc: &Document) -> bool {
    matches(selector, node_id, doc.arena())
}

impl Selector {
    /// Checks if this selector matches the given node in the DOM arena.
    pub fn matches(&self, node_id: NodeId, dom: &Arena) -> bool {
        matches(self, node_id, dom)
    }
}
