#![deny(unsafe_code)]

use crate::ast::{Combinator, Selector};
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

    // Check pseudo-classes
    for pc in &selector.pseudo_classes {
        match pc.as_str() {
            "link" | "any-link" => {
                if !element.tag_name().eq_ignore_ascii_case("a")
                    || element.get_attribute("href").is_none()
                {
                    return false;
                }
            }
            "root" => {
                if !element.tag_name().eq_ignore_ascii_case("html") {
                    return false;
                }
            }
            "first-child" => {
                if node.parent.is_none() {
                    return false;
                }
                let mut prev = node.prev_sibling;
                while let Some(sib_id) = prev {
                    if let Some(sib) = dom.get(sib_id) {
                        if sib.as_element().is_some() {
                            return false;
                        }
                        prev = sib.prev_sibling;
                    } else {
                        break;
                    }
                }
            }
            "last-child" => {
                if node.parent.is_none() {
                    return false;
                }
                let mut next = node.next_sibling;
                while let Some(sib_id) = next {
                    if let Some(sib) = dom.get(sib_id) {
                        if sib.as_element().is_some() {
                            return false;
                        }
                        next = sib.next_sibling;
                    } else {
                        break;
                    }
                }
            }
            // Interactive/visited/pseudo-element states do not match in static DOM evaluation
            _ => return false,
        }
    }

    // Check ancestor combinators
    if let Some((combinator, ref parent_sel)) = selector.parent {
        match combinator {
            Combinator::Child => {
                let Some(parent_id) = node.parent else {
                    return false;
                };
                return matches(parent_sel, parent_id, dom);
            }
            Combinator::Descendant => {
                let mut curr = node.parent;
                while let Some(anc_id) = curr {
                    if matches(parent_sel, anc_id, dom) {
                        return true;
                    }
                    curr = dom.get(anc_id).and_then(|n| n.parent);
                }
                return false;
            }
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
