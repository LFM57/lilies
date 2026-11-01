#![deny(unsafe_code)]

use crate::layout_box::{BoxType, LayoutBox};
use lilies_css::{ComputedStyle, Display, StyleTree};
use lilies_dom::{Arena, NodeId, NodeType};

/// Traverses direct child nodes in allocation order.
fn get_children(arena: &Arena, parent: NodeId) -> Vec<NodeId> {
    let mut children = Vec::new();
    let mut current = arena.get(parent).and_then(|n| n.first_child);
    while let Some(child_id) = current {
        children.push(child_id);
        current = arena.get(child_id).and_then(|n| n.next_sibling);
    }
    children
}

/// Recursively constructs a layout box tree from a DOM node and computed styles.
///
/// Returns `None` if the node or its computed style produces no layout box (e.g., `display: none`,
/// comments, doctypes, or empty text nodes).
pub fn build_layout_tree(
    node_id: NodeId,
    arena: &Arena,
    style_tree: &StyleTree,
) -> Option<LayoutBox> {
    let node = arena.get(node_id)?;

    let (box_type, style) = match &node.node_type {
        NodeType::Element(element) => {
            let style = style_tree
                .get(node_id)
                .cloned()
                .unwrap_or_else(|| ComputedStyle::default_for_tag(element.tag_name()));

            if style.display == Display::None {
                return None;
            }

            let box_type = match style.display {
                Display::Block => BoxType::BlockNode(node_id),
                Display::Inline => BoxType::InlineNode(node_id),
                Display::None => return None,
            };

            (box_type, Some(style))
        }

        NodeType::Document => {
            let style = style_tree.get(node_id).cloned();
            (BoxType::BlockNode(node_id), style)
        }

        NodeType::Text(text) => {
            // Collapse whitespace-only text nodes
            if text.data.trim().is_empty() {
                return None;
            }
            let parent_style = node.parent.and_then(|p| style_tree.get(p));
            let style = style_tree
                .get(node_id)
                .cloned()
                .or_else(|| {
                    parent_style.cloned().map(|mut s| {
                        s.display = Display::Inline;
                        s
                    })
                })
                .or_else(|| Some(ComputedStyle::default()));

            let mut text_box = LayoutBox::new(BoxType::InlineNode(node_id));
            text_box.style = style;
            text_box.text = Some(text.data.clone());
            return Some(text_box);
        }

        NodeType::Comment(_) | NodeType::DocumentType(_) => {
            return None;
        }
    };

    // Recursively build layout boxes for children
    let mut raw_children = Vec::new();
    for child_id in get_children(arena, node_id) {
        if let Some(child_box) = build_layout_tree(child_id, arena, style_tree) {
            raw_children.push(child_box);
        }
    }

    // CSS Normalization: if a block box has any block children, all children must be block.
    // Wrap consecutive inline children into AnonymousBlock boxes.
    let has_block = raw_children.iter().any(|c| c.box_type.is_block());
    let has_inline = raw_children.iter().any(|c| c.box_type.is_inline());

    let children = if has_block && has_inline {
        let mut normalized = Vec::new();
        let mut inline_run = Vec::new();

        for child in raw_children {
            if child.box_type.is_inline() {
                inline_run.push(child);
            } else {
                if !inline_run.is_empty() {
                    let mut anon = LayoutBox::new(BoxType::AnonymousBlock);
                    anon.children = std::mem::take(&mut inline_run);
                    normalized.push(anon);
                }
                normalized.push(child);
            }
        }

        if !inline_run.is_empty() {
            let mut anon = LayoutBox::new(BoxType::AnonymousBlock);
            anon.children = inline_run;
            normalized.push(anon);
        }

        normalized
    } else {
        raw_children
    };

    let mut layout_box = LayoutBox::new(box_type);
    layout_box.children = children;
    layout_box.style = style;
    Some(layout_box)
}
