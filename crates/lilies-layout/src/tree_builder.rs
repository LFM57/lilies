#![deny(unsafe_code)]

use crate::layout_box::{BoxType, LayoutBox};
use lilies_css::{ComputedStyle, Display, LengthOrAuto, StyleTree};
use lilies_dom::{Arena, NodeId, NodeType};
use lilies_net::{DecodedImage, ImageCache};
use std::collections::HashMap;
use std::sync::Arc;

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

/// Helper that parses integer or pixel dimension string (e.g. "120" or "120px").
fn parse_dimension_attr(attr: Option<&str>) -> Option<f32> {
    let s = attr?.trim();
    let s = s.strip_suffix("px").unwrap_or(s).trim();
    s.parse::<f32>().ok().filter(|v| *v >= 0.0)
}

/// Recursively constructs a layout box tree from a DOM node and computed styles without image cache.
pub fn build_layout_tree(
    node_id: NodeId,
    arena: &Arena,
    style_tree: &StyleTree,
) -> Option<LayoutBox> {
    build_layout_tree_with_images(node_id, arena, style_tree, &HashMap::new())
}

/// Recursively constructs a layout box tree using an `ImageCache`.
pub fn build_layout_tree_with_cache(
    node_id: NodeId,
    arena: &Arena,
    style_tree: &StyleTree,
    cache: &ImageCache,
) -> Option<LayoutBox> {
    build_layout_tree_with_images(node_id, arena, style_tree, &cache.snapshot())
}

/// Recursively constructs a layout box tree from a DOM node, computed styles, and an image map.
///
/// Returns `None` if the node or its computed style produces no layout box (e.g., `display: none`,
/// comments, doctypes, or empty text nodes).
pub fn build_layout_tree_with_images(
    node_id: NodeId,
    arena: &Arena,
    style_tree: &StyleTree,
    image_cache: &HashMap<String, Arc<DecodedImage>>,
) -> Option<LayoutBox> {
    let node = arena.get(node_id)?;

    let (box_type, style) = match &node.node_type {
        NodeType::Element(element) => {
            let mut style = style_tree
                .get(node_id)
                .cloned()
                .unwrap_or_else(|| ComputedStyle::default_for_tag(element.tag_name()));

            if style.display == Display::None {
                return None;
            }

            if element.tag_name().eq_ignore_ascii_case("img") {
                let src = element.get_attribute("src").unwrap_or("").trim();
                let maybe_image = if !src.is_empty() {
                    image_cache.get(src).cloned()
                } else {
                    None
                };

                let attr_w = parse_dimension_attr(element.get_attribute("width"));
                let attr_h = parse_dimension_attr(element.get_attribute("height"));
                let css_w = style.width.to_px();
                let css_h = style.height.to_px();
                let specified_w = css_w.or(attr_w);
                let specified_h = css_h.or(attr_h);

                if let Some(image) = maybe_image {
                    let intrinsic_w = image.width as f32;
                    let intrinsic_h = image.height as f32;

                    let (w, h) = match (specified_w, specified_h) {
                        (Some(w), Some(h)) => (w, h),
                        (Some(w), None) => {
                            let h = if intrinsic_w > 0.0 {
                                w * (intrinsic_h / intrinsic_w)
                            } else {
                                intrinsic_h
                            };
                            (w, h)
                        }
                        (None, Some(h)) => {
                            let w = if intrinsic_h > 0.0 {
                                h * (intrinsic_w / intrinsic_h)
                            } else {
                                intrinsic_w
                            };
                            (w, h)
                        }
                        (None, None) => (intrinsic_w, intrinsic_h),
                    };

                    if style.width.is_auto() {
                        style.width = LengthOrAuto::Px(w);
                    }
                    if style.height.is_auto() {
                        style.height = LengthOrAuto::Px(h);
                    }

                    let mut img_box =
                        LayoutBox::new(BoxType::ImageNode(node_id, Arc::clone(&image)));
                    img_box.dimensions.content.width = w;
                    img_box.dimensions.content.height = h;
                    img_box.style = Some(style);
                    return Some(img_box);
                } else {
                    // Missing or failed image
                    let alt = element.get_attribute("alt");
                    let mut alt_w = specified_w;
                    let mut alt_h = specified_h;

                    if let Some(alt_text) = alt {
                        if !alt_text.trim().is_empty() {
                            let text_w = crate::font::measure_text(alt_text, style.font_size);
                            let text_h = crate::font::line_height(style.font_size);
                            alt_w = alt_w.or(Some(text_w));
                            alt_h = alt_h.or(Some(text_h));

                            let box_type = match style.display {
                                Display::Block => BoxType::BlockNode(node_id),
                                _ => BoxType::InlineNode(node_id),
                            };
                            let mut fallback_box =
                                LayoutBox::new(box_type).with_text(alt_text.to_string());
                            if let Some(w) = alt_w {
                                fallback_box.dimensions.content.width = w;
                                if style.width.is_auto() {
                                    style.width = LengthOrAuto::Px(w);
                                }
                            }
                            if let Some(h) = alt_h {
                                fallback_box.dimensions.content.height = h;
                                if style.height.is_auto() {
                                    style.height = LengthOrAuto::Px(h);
                                }
                            }
                            fallback_box.style = Some(style);
                            return Some(fallback_box);
                        }
                    }

                    let w = specified_w.unwrap_or(0.0);
                    let h = specified_h.unwrap_or(0.0);
                    let box_type = match style.display {
                        Display::Block => BoxType::BlockNode(node_id),
                        _ => BoxType::InlineNode(node_id),
                    };
                    let mut fallback_box = LayoutBox::new(box_type);
                    fallback_box.dimensions.content.width = w;
                    fallback_box.dimensions.content.height = h;
                    if style.width.is_auto() && w > 0.0 {
                        style.width = LengthOrAuto::Px(w);
                    }
                    if style.height.is_auto() && h > 0.0 {
                        style.height = LengthOrAuto::Px(h);
                    }
                    fallback_box.style = Some(style);
                    return Some(fallback_box);
                }
            }

            let box_type = match style.display {
                Display::Block | Display::Flex => BoxType::BlockNode(node_id),
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
        if let Some(child_box) =
            build_layout_tree_with_images(child_id, arena, style_tree, image_cache)
        {
            raw_children.push(child_box);
        }
    }

    // CSS Normalization: if a block box has any block children, all children must be block.
    // Wrap consecutive inline children into AnonymousBlock boxes (except in flex containers, where all items are flex items).
    let is_flex = style.as_ref().is_some_and(|s| s.display == Display::Flex);
    let has_block = raw_children.iter().any(|c| c.is_block());
    let has_inline = raw_children.iter().any(|c| c.is_inline());

    let children = if !is_flex && has_block && has_inline {
        let mut normalized = Vec::new();
        let mut inline_run = Vec::new();

        for child in raw_children {
            if child.is_inline() {
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
