#![deny(unsafe_code)]

use crate::layout_box::LayoutBox;
use lilies_dom::{Arena, NodeId};

/// Performs hit-testing to find the innermost DOM `NodeId` under coordinate `(x, y)`.
///
/// Returns `None` if `(x, y)` is outside `layout_box.dimensions.border_box()`.
/// Children are searched recursively in reverse paint order (back to front, topmost first).
pub fn hit_test(layout_box: &LayoutBox, x: f32, y: f32) -> Option<NodeId> {
    let border_box = layout_box.dimensions.border_box();
    if !border_box.contains(x, y) {
        return None;
    }

    // Search children in reverse order (topmost painted boxes first)
    for child in layout_box.children.iter().rev() {
        if let Some(innermost) = hit_test(child, x, y) {
            return Some(innermost);
        }
    }

    // Return this box's node_id if not anonymous
    layout_box.node_id()
}

/// Walks up the DOM tree from `node_id` to the root.
///
/// If any enclosing ancestor node (or `node_id` itself) is an `<a href="...">` element,
/// returns the `href` attribute value.
pub fn find_parent_link(node_id: NodeId, arena: &Arena) -> Option<String> {
    let mut current = Some(node_id);
    while let Some(id) = current {
        let node = arena.get(id)?;
        if let Some(element) = node.as_element() {
            if element.tag_name().eq_ignore_ascii_case("a") {
                if let Some(href) = element.get_attribute("href") {
                    return Some(href.to_string());
                }
            }
        }
        current = node.parent;
    }
    None
}
