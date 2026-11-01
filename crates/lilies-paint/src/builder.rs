#![deny(unsafe_code)]

use crate::display_item::{DisplayItem, DisplayList};
use lilies_css::Color;
use lilies_layout::LayoutBox;

/// Constructs a `DisplayList` by traversing a `LayoutBox` tree following CSS 2.1 paint order.
///
/// Paint order for each box:
/// 1. Background (`dimensions.padding_box()`)
/// 2. Borders (`dimensions.border_box()`)
/// 3. Children (front to back)
pub fn build_display_list(layout_root: &LayoutBox) -> DisplayList {
    let mut display_list = DisplayList::new();
    render_layout_box(layout_root, &mut display_list);
    display_list
}

fn render_layout_box(layout_box: &LayoutBox, list: &mut DisplayList) {
    render_background(layout_box, list);
    render_borders(layout_box, list);

    for child in &layout_box.children {
        render_layout_box(child, list);
    }
}

fn render_background(layout_box: &LayoutBox, list: &mut DisplayList) {
    if let Some(style) = layout_box.get_style() {
        if style.background_color.a > 0 {
            let padding_box = layout_box.dimensions.padding_box();
            if padding_box.width > 0.0 && padding_box.height > 0.0 {
                list.push(DisplayItem::SolidColor {
                    rect: padding_box,
                    color: style.background_color,
                });
            }
        }
    }
}

fn render_borders(layout_box: &LayoutBox, list: &mut DisplayList) {
    let border = layout_box.dimensions.border;
    if border.top > 0.0 || border.right > 0.0 || border.bottom > 0.0 || border.left > 0.0 {
        let border_box = layout_box.dimensions.border_box();
        if border_box.width > 0.0 && border_box.height > 0.0 {
            let color = layout_box.get_style().map_or(Color::BLACK, |s| s.color);
            list.push(DisplayItem::Border {
                rect: border_box,
                widths: border,
                color,
            });
        }
    }
}
