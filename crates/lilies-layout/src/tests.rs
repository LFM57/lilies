#![deny(unsafe_code)]

use crate::{build_layout_tree, layout, BoxType, Dimensions, EdgeSizes, LayoutBox, Rect};
use lilies_css::{
    compute_styles, parse_stylesheet, AlignItems, ComputedStyle, Display, FlexDirection,
    JustifyContent, LengthOrAuto, StyleTree,
};
use lilies_dom::{Document, IndexMap, NodeId};
use lilies_html::parse_html;

#[test]
fn test_dimensions_and_box_model_helpers() {
    let mut dims = Dimensions::new(Rect::new(10.0, 20.0, 100.0, 50.0));
    dims.padding = EdgeSizes::new(5.0, 10.0, 15.0, 20.0);
    dims.border = EdgeSizes::new(1.0, 2.0, 3.0, 4.0);
    dims.margin = EdgeSizes::new(8.0, 6.0, 4.0, 2.0);

    // Padding box:
    // x = 10 - 20 = -10
    // y = 20 - 5 = 15
    // width = 100 + 20 + 10 = 130
    // height = 50 + 5 + 15 = 70
    let p_box = dims.padding_box();
    assert_eq!(p_box, Rect::new(-10.0, 15.0, 130.0, 70.0));

    // Border box:
    // x = -10 - 4 = -14
    // y = 15 - 1 = 14
    // width = 130 + 4 + 2 = 136
    // height = 70 + 1 + 3 = 74
    let b_box = dims.border_box();
    assert_eq!(b_box, Rect::new(-14.0, 14.0, 136.0, 74.0));

    // Margin box:
    // x = -14 - 2 = -16
    // y = 14 - 8 = 6
    // width = 136 + 2 + 6 = 144
    // height = 74 + 8 + 4 = 86
    let m_box = dims.margin_box();
    assert_eq!(m_box, Rect::new(-16.0, 6.0, 144.0, 86.0));
}

#[test]
fn test_display_none_suppresses_box_generation() {
    let mut doc = Document::new();
    let root = doc.root();

    let parent = doc.create_element("div", IndexMap::new());
    let visible_child = doc.create_element("p", IndexMap::new());
    let hidden_child = doc.create_element("span", IndexMap::new());
    let subchild = doc.create_element("b", IndexMap::new());

    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, visible_child).unwrap();
    doc.append_child(parent, hidden_child).unwrap();
    doc.append_child(hidden_child, subchild).unwrap();

    let mut style_tree = StyleTree::new();

    let parent_style = ComputedStyle {
        display: Display::Block,
        ..Default::default()
    };
    style_tree.insert(parent, parent_style);

    let visible_style = ComputedStyle {
        display: Display::Block,
        ..Default::default()
    };
    style_tree.insert(visible_child, visible_style);

    let hidden_style = ComputedStyle {
        display: Display::None,
        ..Default::default()
    };
    style_tree.insert(hidden_child, hidden_style);

    // Hidden child should return None directly
    assert!(build_layout_tree(hidden_child, doc.arena(), &style_tree).is_none());

    // Parent layout tree should only contain the visible child
    let layout_tree = build_layout_tree(parent, doc.arena(), &style_tree).unwrap();
    assert_eq!(layout_tree.box_type, BoxType::BlockNode(parent));
    assert_eq!(layout_tree.children.len(), 1);
    assert_eq!(
        layout_tree.children[0].box_type,
        BoxType::BlockNode(visible_child)
    );
}

#[test]
fn test_anonymous_block_grouping_for_mixed_children() {
    let mut doc = Document::new();
    let root = doc.root();

    let container = doc.create_element("div", IndexMap::new());
    let inline1 = doc.create_element("span", IndexMap::new());
    let inline2 = doc.create_element("span", IndexMap::new());
    let block1 = doc.create_element("div", IndexMap::new());
    let inline3 = doc.create_element("span", IndexMap::new());

    doc.append_child(root, container).unwrap();
    doc.append_child(container, inline1).unwrap();
    doc.append_child(container, inline2).unwrap();
    doc.append_child(container, block1).unwrap();
    doc.append_child(container, inline3).unwrap();

    let mut style_tree = StyleTree::new();
    let block_style = ComputedStyle {
        display: Display::Block,
        ..Default::default()
    };

    let inline_style = ComputedStyle {
        display: Display::Inline,
        ..Default::default()
    };

    style_tree.insert(container, block_style.clone());
    style_tree.insert(inline1, inline_style.clone());
    style_tree.insert(inline2, inline_style.clone());
    style_tree.insert(block1, block_style);
    style_tree.insert(inline3, inline_style);

    let layout_box = build_layout_tree(container, doc.arena(), &style_tree).unwrap();
    assert_eq!(layout_box.box_type, BoxType::BlockNode(container));

    // Sibling inline children are grouped into anonymous blocks when next to block boxes:
    // [AnonymousBlock([inline1, inline2]), BlockNode(block1), AnonymousBlock([inline3])]
    assert_eq!(layout_box.children.len(), 3);

    // First child: AnonymousBlock containing inline1 and inline2
    assert_eq!(layout_box.children[0].box_type, BoxType::AnonymousBlock);
    assert_eq!(layout_box.children[0].children.len(), 2);
    assert_eq!(
        layout_box.children[0].children[0].box_type,
        BoxType::InlineNode(inline1)
    );
    assert_eq!(
        layout_box.children[0].children[1].box_type,
        BoxType::InlineNode(inline2)
    );

    // Second child: BlockNode(block1)
    assert_eq!(layout_box.children[1].box_type, BoxType::BlockNode(block1));

    // Third child: AnonymousBlock containing inline3
    assert_eq!(layout_box.children[2].box_type, BoxType::AnonymousBlock);
    assert_eq!(layout_box.children[2].children.len(), 1);
    assert_eq!(
        layout_box.children[2].children[0].box_type,
        BoxType::InlineNode(inline3)
    );
}

#[test]
fn test_all_inline_children_no_anonymous_block() {
    let mut doc = Document::new();
    let container = doc.create_element("p", IndexMap::new());
    let span1 = doc.create_element("span", IndexMap::new());
    let span2 = doc.create_element("span", IndexMap::new());

    doc.append_child(container, span1).unwrap();
    doc.append_child(container, span2).unwrap();

    let mut style_tree = StyleTree::new();
    let block_style = ComputedStyle {
        display: Display::Block,
        ..Default::default()
    };
    let inline_style = ComputedStyle {
        display: Display::Inline,
        ..Default::default()
    };

    style_tree.insert(container, block_style);
    style_tree.insert(span1, inline_style.clone());
    style_tree.insert(span2, inline_style);

    let layout_box = build_layout_tree(container, doc.arena(), &style_tree).unwrap();
    // Since there are no block siblings, inlines are not wrapped in anonymous blocks
    assert_eq!(layout_box.children.len(), 2);
    assert_eq!(layout_box.children[0].box_type, BoxType::InlineNode(span1));
    assert_eq!(layout_box.children[1].box_type, BoxType::InlineNode(span2));
}

#[test]
fn test_child_width_auto_expands_to_fill_parent() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    let mut child = LayoutBox::new(BoxType::AnonymousBlock);
    child.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Auto,
        margin_left: LengthOrAuto::Px(15.0),
        margin_right: LengthOrAuto::Px(25.0),
        padding_left: 30.0,
        padding_right: 10.0,
        ..Default::default()
    });

    let mut root = LayoutBox::new(BoxType::AnonymousBlock);
    root.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(500.0),
        ..Default::default()
    });
    root.children.push(child);

    layout(&mut root, viewport);

    assert_eq!(root.dimensions.content.width, 500.0);

    let laid_out_child = &root.children[0];
    // Containing block width = 500
    // Child margins & paddings = 15 (ml) + 30 (pl) + 10 (pr) + 25 (mr) = 80
    // Expected content width = 500 - 80 = 420
    assert_eq!(laid_out_child.dimensions.content.width, 420.0);
    assert_eq!(laid_out_child.dimensions.margin.left, 15.0);
    assert_eq!(laid_out_child.dimensions.margin.right, 25.0);
    assert_eq!(laid_out_child.dimensions.padding.left, 30.0);
    assert_eq!(laid_out_child.dimensions.padding.right, 10.0);
    assert_eq!(laid_out_child.dimensions.margin_box().width, 500.0);
}

#[test]
fn test_margin_auto_horizontal_centering() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    let mut root = LayoutBox::new(BoxType::AnonymousBlock);
    root.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(600.0),
        ..Default::default()
    });

    let mut child = LayoutBox::new(BoxType::AnonymousBlock);
    child.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(200.0),
        margin_left: LengthOrAuto::Auto,
        margin_right: LengthOrAuto::Auto,
        ..Default::default()
    });

    root.children.push(child);
    layout(&mut root, viewport);

    let laid_out_child = &root.children[0];
    // Underflow = 600 - 200 = 400
    // Centered margins: 400 / 2 = 200 each
    assert_eq!(laid_out_child.dimensions.content.width, 200.0);
    assert_eq!(laid_out_child.dimensions.margin.left, 200.0);
    assert_eq!(laid_out_child.dimensions.margin.right, 200.0);
    // Position: root content.x (0) + margin_left (200) = 200
    assert_eq!(laid_out_child.dimensions.content.x, 200.0);
}

#[test]
fn test_margin_auto_single_side() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    let mut root = LayoutBox::new(BoxType::AnonymousBlock);
    root.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(500.0),
        ..Default::default()
    });

    // Test margin-left: auto (flush right)
    let mut child1 = LayoutBox::new(BoxType::AnonymousBlock);
    child1.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(150.0),
        margin_left: LengthOrAuto::Auto,
        margin_right: LengthOrAuto::Px(20.0),
        ..Default::default()
    });

    root.children.push(child1);
    layout(&mut root, viewport);

    let laid_out_c1 = &root.children[0];
    // Underflow = 500 - (150 + 20) = 330
    assert_eq!(laid_out_c1.dimensions.content.width, 150.0);
    assert_eq!(laid_out_c1.dimensions.margin.left, 330.0);
    assert_eq!(laid_out_c1.dimensions.margin.right, 20.0);
    assert_eq!(laid_out_c1.dimensions.margin_box().width, 500.0);
}

#[test]
fn test_overconstrained_width_adjusts_margin_right() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    let mut root = LayoutBox::new(BoxType::AnonymousBlock);
    root.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(600.0),
        ..Default::default()
    });

    let mut child = LayoutBox::new(BoxType::AnonymousBlock);
    child.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(200.0),
        margin_left: LengthOrAuto::Px(50.0),
        margin_right: LengthOrAuto::Px(50.0), // Overconstrained: total = 300 < 600
        ..Default::default()
    });

    root.children.push(child);
    layout(&mut root, viewport);

    let laid_out_child = &root.children[0];
    // Underflow = 600 - 300 = 300. In LTR, margin-right absorbs underflow: 50 + 300 = 350
    assert_eq!(laid_out_child.dimensions.margin.left, 50.0);
    assert_eq!(laid_out_child.dimensions.margin.right, 350.0);
    assert_eq!(laid_out_child.dimensions.margin_box().width, 600.0);
}

#[test]
fn test_vertical_stacking_sibling_blocks() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    let mut root = LayoutBox::new(BoxType::AnonymousBlock);
    root.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(800.0),
        ..Default::default()
    });

    // Box A: height 100, margin-top 10, margin-bottom 20, padding-top 5, padding-bottom 5
    let mut box_a = LayoutBox::new(BoxType::AnonymousBlock);
    box_a.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(100.0),
        margin_top: LengthOrAuto::Px(10.0),
        margin_bottom: LengthOrAuto::Px(20.0),
        padding_top: 5.0,
        padding_bottom: 5.0,
        ..Default::default()
    });

    // Box B: height 60, margin-top 15, margin-bottom 10, padding-top 0, padding-bottom 0
    let mut box_b = LayoutBox::new(BoxType::AnonymousBlock);
    box_b.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(60.0),
        margin_top: LengthOrAuto::Px(15.0),
        margin_bottom: LengthOrAuto::Px(10.0),
        ..Default::default()
    });

    root.children.push(box_a);
    root.children.push(box_b);

    layout(&mut root, viewport);

    let laid_a = &root.children[0];
    let laid_b = &root.children[1];

    // Box A content.y = root.y (0) + margin_top (10) + padding_top (5) = 15
    assert_eq!(laid_a.dimensions.content.y, 15.0);
    assert_eq!(laid_a.dimensions.content.height, 100.0);

    // Box A margin box:
    // y = 15 - 5 - 10 = 0
    // height = 10 (mt) + 5 (pt) + 100 (h) + 5 (pb) + 20 (mb) = 140
    let a_margin_box = laid_a.dimensions.margin_box();
    assert_eq!(a_margin_box.y, 0.0);
    assert_eq!(a_margin_box.height, 140.0);

    // Bottom margin edge of Box A = 0 + 140 = 140
    let a_bottom_margin_edge = a_margin_box.y + a_margin_box.height;
    assert_eq!(a_bottom_margin_edge, 140.0);

    // Box B margin box top should exactly align with Box A's bottom margin edge (140)
    let b_margin_box = laid_b.dimensions.margin_box();
    assert_eq!(b_margin_box.y, a_bottom_margin_edge);

    // Box B content.y = 140 + margin_top (15) + padding_top (0) = 155
    assert_eq!(laid_b.dimensions.content.y, 155.0);
    assert!(laid_b.dimensions.content.y >= a_bottom_margin_edge);

    // Box B margin box height = 15 (mt) + 60 (h) + 10 (mb) = 85
    assert_eq!(b_margin_box.height, 85.0);

    // Total content height of parent root = 140 + 85 = 225
    assert_eq!(root.dimensions.content.height, 225.0);
}

#[test]
fn test_explicit_width_and_height_vs_auto() {
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    // Case 1: Explicit height vs auto height with children
    let mut auto_parent = LayoutBox::new(BoxType::AnonymousBlock);
    auto_parent.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Auto,
        ..Default::default()
    });

    let mut child1 = LayoutBox::new(BoxType::AnonymousBlock);
    child1.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(45.0),
        ..Default::default()
    });

    let mut child2 = LayoutBox::new(BoxType::AnonymousBlock);
    child2.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(55.0),
        ..Default::default()
    });

    auto_parent.children.push(child1);
    auto_parent.children.push(child2);

    layout(&mut auto_parent, viewport);
    // Height: auto => sum of children = 45 + 55 = 100
    assert_eq!(auto_parent.dimensions.content.height, 100.0);

    // Case 2: Explicit height overrides children height
    let mut explicit_parent = LayoutBox::new(BoxType::AnonymousBlock);
    explicit_parent.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(75.0),
        ..Default::default()
    });

    let mut c3 = LayoutBox::new(BoxType::AnonymousBlock);
    c3.style = Some(ComputedStyle {
        display: Display::Block,
        height: LengthOrAuto::Px(150.0),
        ..Default::default()
    });
    explicit_parent.children.push(c3);

    layout(&mut explicit_parent, viewport);
    // Explicit height of 75 takes precedence over child's 150
    assert_eq!(explicit_parent.dimensions.content.height, 75.0);
}

#[test]
fn test_tree_layout_on_simulated_dom_and_style_tree() {
    let html = r#"<!DOCTYPE html>
<html>
  <body>
    <header class="hdr">Header text</header>
    <div id="main" class="container">
      <p>Paragraph inside container</p>
    </div>
    <footer class="ftr">Footer text</footer>
  </body>
</html>"#;

    let css = r#"
      html {
        width: 800px;
        margin: 0px;
        padding: 0px;
      }
      body {
        margin: 10px;
        padding: 10px;
      }
      .hdr {
        height: 50px;
        margin-bottom: 20px;
      }
      .container {
        width: 600px;
        padding: 15px;
        margin: auto;
      }
      p {
        height: 40px;
        margin: 5px;
      }
      .ftr {
        height: 30px;
        margin-top: 25px;
      }
    "#;

    let doc = parse_html(html);
    let stylesheet = parse_stylesheet(css).unwrap();
    let style_tree = compute_styles(&doc, &stylesheet);

    // Find html element
    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    });

    let html_id = html_id.expect("html element should exist");

    let mut layout_root =
        build_layout_tree(html_id, doc.arena(), &style_tree).expect("Failed to build layout tree");

    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    layout(&mut layout_root, viewport);

    // Verify html dimensions
    assert_eq!(layout_root.dimensions.content.width, 800.0);

    // Verify body is inside html
    assert_eq!(layout_root.children.len(), 1);
    let body_box = &layout_root.children[0];
    assert_eq!(body_box.dimensions.margin.top, 10.0);
    assert_eq!(body_box.dimensions.padding.top, 10.0);
    assert_eq!(body_box.dimensions.content.x, 20.0); // body margin 10 + padding 10
    assert_eq!(body_box.dimensions.content.y, 20.0);

    // Body should have 3 children: header, container, footer
    assert_eq!(body_box.children.len(), 3);
    let header_box = &body_box.children[0];
    let container_box = &body_box.children[1];
    let footer_box = &body_box.children[2];

    assert_eq!(header_box.dimensions.content.height, 50.0);
    assert_eq!(container_box.dimensions.content.width, 600.0);
    assert_eq!(footer_box.dimensions.content.height, 30.0);

    // Verify vertical ordering:
    // Header comes before Container, Container comes before Footer
    assert!(header_box.dimensions.margin_box().y < container_box.dimensions.margin_box().y);
    assert!(container_box.dimensions.margin_box().y < footer_box.dimensions.margin_box().y);
}

#[test]
fn test_doctype_and_comments_no_box() {
    let mut doc = Document::new();
    let doctype = doc.create_doctype("html", None, None);
    let comment = doc.create_comment("a comment");
    let style_tree = StyleTree::new();

    assert!(build_layout_tree(doctype, doc.arena(), &style_tree).is_none());
    assert!(build_layout_tree(comment, doc.arena(), &style_tree).is_none());
}

#[test]
fn test_font_text_measurement() {
    use crate::font::{font_ascent, line_height, measure_char, measure_text};

    let w_a = measure_char('a', 16.0);
    let w_m = measure_char('m', 16.0);
    let w_space = measure_char(' ', 16.0);

    assert!(w_a > 0.0);
    assert!(w_m > w_a);
    assert!(w_space > 0.0);

    let text_16 = "lilies browser";
    let width_16 = measure_text(text_16, 16.0);
    let width_32 = measure_text(text_16, 32.0);

    assert!(width_16 > 0.0);
    // At double font size, horizontal advance should roughly double
    assert!((width_32 - (width_16 * 2.0)).abs() < 5.0);

    assert_eq!(line_height(16.0), 19.2);
    assert_eq!(line_height(32.0), 38.4);
    assert!(font_ascent(16.0) > 0.0);
}

#[test]
fn test_word_wrapping_width_boundaries() {
    use crate::font::{measure_char, measure_text, wrap_text};

    let text = "apple banana cherry date";
    let font_size = 16.0;

    // 1. Unconstrained: all words fit on one line
    let lines_unconstrained = wrap_text(text, font_size, 1000.0, 10.0, 20.0);
    assert_eq!(lines_unconstrained.len(), 1);
    assert_eq!(lines_unconstrained[0].text, "apple banana cherry date");
    assert_eq!(lines_unconstrained[0].rect.x, 10.0);
    assert_eq!(lines_unconstrained[0].rect.y, 20.0);

    // 2. Exact boundary between 2 words and 3 words:
    let w_apple = measure_text("apple", font_size);
    let w_banana = measure_text("banana", font_size);
    let w_cherry = measure_text("cherry", font_size);
    let w_space = measure_char(' ', font_size);

    let two_words_width = w_apple + w_space + w_banana;
    let three_words_width = two_words_width + w_space + w_cherry;

    // Boundary: exactly fits two words
    let lines_two = wrap_text(text, font_size, two_words_width + 0.1, 0.0, 0.0);
    assert_eq!(lines_two[0].text, "apple banana");

    // Just smaller than two words: apple wraps banana to line 2
    let lines_tight = wrap_text(text, font_size, two_words_width - 1.0, 0.0, 0.0);
    assert_eq!(lines_tight[0].text, "apple");
    assert_eq!(lines_tight[1].text, "banana");

    // Fits 3 words
    let lines_three = wrap_text(text, font_size, three_words_width + 0.1, 0.0, 0.0);
    assert_eq!(lines_three[0].text, "apple banana cherry");
    assert_eq!(lines_three[1].text, "date");

    // 3. Single oversized word: does not crash or loop infinitely
    let long_word = "supercalifragilisticexpialidocious";
    let lines_long = wrap_text(long_word, font_size, 50.0, 0.0, 0.0);
    assert_eq!(lines_long.len(), 1);
    assert_eq!(lines_long[0].text, long_word);

    // 4. Empty and whitespace-only text
    assert!(wrap_text("", font_size, 200.0, 0.0, 0.0).is_empty());
    assert!(wrap_text("   \n\t  ", font_size, 200.0, 0.0, 0.0).is_empty());
}

#[test]
fn test_text_node_layout_and_height_calculation() {
    let html = r#"<html><body><div style="width: 200px;"><p>One two three four five six seven eight nine ten eleven twelve</p></div></body></html>"#;
    let doc = parse_html(html);
    let stylesheet = lilies_css::StyleSheet::new();
    let styles = compute_styles(&doc, &stylesheet);

    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("html element");

    let mut root_box = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    layout(&mut root_box, viewport);

    let body = &root_box.children[0];
    let div = &body.children[0];
    let p = &div.children[0];
    let text_box = &p.children[0];

    assert!(!text_box.lines.is_empty());
    assert!(text_box.lines.len() >= 2);

    let expected_height = text_box.lines.len() as f32 * (16.0 * 1.2);
    assert_eq!(text_box.dimensions.content.height, expected_height);
    assert_eq!(p.dimensions.content.height, expected_height);
    assert_eq!(div.dimensions.content.height, expected_height);

    // Check line positioning is monotonically increasing in y
    for i in 1..text_box.lines.len() {
        assert!(text_box.lines[i].rect.y > text_box.lines[i - 1].rect.y);
        assert_eq!(
            text_box.lines[i].rect.y,
            text_box.lines[i - 1].rect.y + (16.0 * 1.2)
        );
    }
}

#[test]
fn test_hit_testing_nested_boxes() {
    use crate::hit_test::hit_test;
    use lilies_dom::NodeId;

    let id_root = NodeId(1);
    let id_child1 = NodeId(2);
    let id_child2 = NodeId(3);
    let id_grandchild = NodeId(4);

    let mut grandchild = LayoutBox::new(BoxType::InlineNode(id_grandchild));
    grandchild.dimensions = Dimensions::new(Rect::new(20.0, 20.0, 50.0, 30.0));

    let mut child1 = LayoutBox::new(BoxType::BlockNode(id_child1));
    child1.dimensions = Dimensions::new(Rect::new(10.0, 10.0, 150.0, 100.0));
    child1.children.push(grandchild);

    let mut child2 = LayoutBox::new(BoxType::BlockNode(id_child2));
    child2.dimensions = Dimensions::new(Rect::new(200.0, 10.0, 150.0, 100.0));

    let mut root = LayoutBox::new(BoxType::BlockNode(id_root));
    root.dimensions = Dimensions::new(Rect::new(0.0, 0.0, 400.0, 300.0));
    root.children.push(child1);
    root.children.push(child2);

    // 1. Point inside innermost grandchild
    assert_eq!(hit_test(&root, 30.0, 30.0), Some(id_grandchild));

    // 2. Point inside child1 but outside grandchild
    assert_eq!(hit_test(&root, 15.0, 15.0), Some(id_child1));

    // 3. Point inside child2
    assert_eq!(hit_test(&root, 250.0, 50.0), Some(id_child2));

    // 4. Point inside root margin/padding outside children
    assert_eq!(hit_test(&root, 5.0, 5.0), Some(id_root));

    // 5. Point completely outside root
    assert_eq!(hit_test(&root, 500.0, 500.0), None);
}

#[test]
fn test_hit_testing_topmost_overlapping_boxes() {
    use crate::hit_test::hit_test;
    use lilies_dom::NodeId;

    let id_root = NodeId(1);
    let id_first = NodeId(2);
    let id_second = NodeId(3);

    let mut first = LayoutBox::new(BoxType::BlockNode(id_first));
    first.dimensions = Dimensions::new(Rect::new(10.0, 10.0, 100.0, 100.0));

    let mut second = LayoutBox::new(BoxType::BlockNode(id_second));
    second.dimensions = Dimensions::new(Rect::new(50.0, 50.0, 100.0, 100.0));

    let mut root = LayoutBox::new(BoxType::BlockNode(id_root));
    root.dimensions = Dimensions::new(Rect::new(0.0, 0.0, 300.0, 300.0));
    root.children.push(first);
    root.children.push(second);

    // Point in overlap region (60, 60): second is painted later (topmost), so it must be hit
    assert_eq!(hit_test(&root, 60.0, 60.0), Some(id_second));

    // Point in only first region (20, 20): hits first
    assert_eq!(hit_test(&root, 20.0, 20.0), Some(id_first));
}

#[test]
fn test_find_parent_link_traversal() {
    use crate::hit_test::find_parent_link;

    let html =
        r#"<div><p><a href="https://example.com/about"><span>Click here</span></a></p></div>"#;
    let doc = parse_html(html);

    // Find span, a, and p nodes
    let mut span_id = None;
    let mut a_id = None;
    let mut p_id = None;

    for node in doc.arena().iter() {
        if let Some(el) = node.as_element() {
            match el.tag_name() {
                "span" => span_id = Some(node.id),
                "a" => a_id = Some(node.id),
                "p" => p_id = Some(node.id),
                _ => {}
            }
        }
    }

    let span_id = span_id.expect("span element");
    let a_id = a_id.expect("a element");
    let p_id = p_id.expect("p element");

    // Enclosing link from grandchild span
    assert_eq!(
        find_parent_link(span_id, doc.arena()),
        Some("https://example.com/about".to_string())
    );

    // Link from anchor itself
    assert_eq!(
        find_parent_link(a_id, doc.arena()),
        Some("https://example.com/about".to_string())
    );

    // Paragraph does not have an enclosing link
    assert_eq!(find_parent_link(p_id, doc.arena()), None);
}

#[test]
fn test_max_width_clamping_and_centering_with_margin_auto() {
    use lilies_css::{ComputedStyle, LengthOrAuto};
    use lilies_dom::NodeId;

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));

    // Case 1: width: auto, max-width: 400px, margin: auto in 800px container
    let style1 = ComputedStyle {
        width: LengthOrAuto::Auto,
        max_width: Some(400.0),
        margin_left: LengthOrAuto::Auto,
        margin_right: LengthOrAuto::Auto,
        ..Default::default()
    };
    let mut box1 = LayoutBox::new(BoxType::BlockNode(NodeId(1))).with_style(style1.clone());
    box1.calculate_block_width(&cb);
    box1.calculate_block_position(&cb);

    assert_eq!(box1.dimensions.content.width, 400.0);
    assert_eq!(box1.dimensions.margin.left, 200.0);
    assert_eq!(box1.dimensions.margin.right, 200.0);
    assert_eq!(box1.dimensions.content.x, 200.0);

    // Case 2: Fixed width: 600px clamped to max-width: 400px, margin: auto
    let style2 = ComputedStyle {
        width: LengthOrAuto::Px(600.0),
        max_width: Some(400.0),
        margin_left: LengthOrAuto::Auto,
        margin_right: LengthOrAuto::Auto,
        ..Default::default()
    };
    let mut box2 = LayoutBox::new(BoxType::BlockNode(NodeId(2))).with_style(style2);
    box2.calculate_block_width(&cb);
    box2.calculate_block_position(&cb);

    assert_eq!(box2.dimensions.content.width, 400.0);
    assert_eq!(box2.dimensions.margin.left, 200.0);
    assert_eq!(box2.dimensions.margin.right, 200.0);
    assert_eq!(box2.dimensions.content.x, 200.0);

    // Case 3: Narrow container (300px) smaller than max-width (400px)
    let narrow_cb = Dimensions::new(Rect::new(0.0, 0.0, 300.0, 600.0));
    let mut box3 = LayoutBox::new(BoxType::BlockNode(NodeId(3))).with_style(style1);
    box3.calculate_block_width(&narrow_cb);
    box3.calculate_block_position(&narrow_cb);

    // 300px <= 400px, so not clamped
    assert_eq!(box3.dimensions.content.width, 300.0);
    assert_eq!(box3.dimensions.margin.left, 0.0);
    assert_eq!(box3.dimensions.margin.right, 0.0);
    assert_eq!(box3.dimensions.content.x, 0.0);
}

#[test]
fn test_text_align_center_and_right_line_coordinate_positioning() {
    use crate::font::measure_text;
    use lilies_css::{ComputedStyle, TextAlign};
    use lilies_dom::NodeId;

    let cb = Dimensions::new(Rect::new(50.0, 20.0, 400.0, 300.0));
    let text = "Hello Lilies Browser".to_string();
    let font_size = 16.0;
    let expected_text_width = measure_text(&text, font_size);
    let space = cb.content.width - expected_text_width;

    // Center alignment
    let style_center = ComputedStyle {
        font_size,
        text_align: TextAlign::Center,
        ..Default::default()
    };
    let mut box_center = LayoutBox::new(BoxType::InlineNode(NodeId(1)))
        .with_text(text.clone())
        .with_style(style_center);
    box_center.layout_inline(cb);

    assert_eq!(box_center.lines.len(), 1);
    let line_center = &box_center.lines[0];
    let expected_center_x = cb.content.x + space / 2.0;
    assert!(
        (line_center.rect.x - expected_center_x).abs() < 0.01,
        "Center aligned line x was {}, expected {}",
        line_center.rect.x,
        expected_center_x
    );

    // Right alignment
    let style_right = ComputedStyle {
        font_size,
        text_align: TextAlign::Right,
        ..Default::default()
    };
    let mut box_right = LayoutBox::new(BoxType::InlineNode(NodeId(2)))
        .with_text(text.clone())
        .with_style(style_right);
    box_right.layout_inline(cb);

    assert_eq!(box_right.lines.len(), 1);
    let line_right = &box_right.lines[0];
    let expected_right_x = cb.content.x + space;
    assert!(
        (line_right.rect.x - expected_right_x).abs() < 0.01,
        "Right aligned line x was {}, expected {}",
        line_right.rect.x,
        expected_right_x
    );

    // Left alignment
    let style_left = ComputedStyle {
        font_size,
        text_align: TextAlign::Left,
        ..Default::default()
    };
    let mut box_left = LayoutBox::new(BoxType::InlineNode(NodeId(3)))
        .with_text(text)
        .with_style(style_left);
    box_left.layout_inline(cb);

    assert_eq!(box_left.lines.len(), 1);
    let line_left = &box_left.lines[0];
    assert_eq!(line_left.rect.x, cb.content.x);
}

#[test]
fn test_image_layout_aspect_ratio_explicit_width() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(200, 100, vec![0; 200 * 100 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Px(100.0),
        height: lilies_css::LengthOrAuto::Auto,
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 100.0);
    assert_eq!(img_box.dimensions.content.height, 50.0);
}

#[test]
fn test_image_layout_aspect_ratio_explicit_height() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(200, 100, vec![0; 200 * 100 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Auto,
        height: lilies_css::LengthOrAuto::Px(80.0),
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 160.0);
    assert_eq!(img_box.dimensions.content.height, 80.0);
}

#[test]
fn test_image_layout_both_explicit_dimensions() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(200, 100, vec![0; 200 * 100 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Px(150.0),
        height: lilies_css::LengthOrAuto::Px(150.0),
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 150.0);
    assert_eq!(img_box.dimensions.content.height, 150.0);
}

#[test]
fn test_image_layout_intrinsic_dimensions_when_neither_specified() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(200, 100, vec![0; 200 * 100 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Auto,
        height: lilies_css::LengthOrAuto::Auto,
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 200.0);
    assert_eq!(img_box.dimensions.content.height, 100.0);
}

#[test]
fn test_image_layout_max_width_preserves_aspect_ratio() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(400, 200, vec![0; 400 * 200 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Px(400.0),
        height: lilies_css::LengthOrAuto::Auto,
        max_width: Some(250.0),
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 500.0, 500.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 250.0);
    assert_eq!(img_box.dimensions.content.height, 125.0);
}

#[test]
fn test_image_layout_margin_auto_horizontal_centering() {
    let img = std::sync::Arc::new(crate::DecodedImage::new(200, 100, vec![0; 200 * 100 * 4]));
    let style = ComputedStyle {
        width: lilies_css::LengthOrAuto::Px(200.0),
        height: lilies_css::LengthOrAuto::Px(100.0),
        margin_left: lilies_css::LengthOrAuto::Auto,
        margin_right: lilies_css::LengthOrAuto::Auto,
        ..Default::default()
    };
    let mut img_box = LayoutBox::new(BoxType::ImageNode(NodeId(1), img)).with_style(style);
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 600.0, 400.0));
    img_box.layout(cb);

    assert_eq!(img_box.dimensions.content.width, 200.0);
    assert_eq!(img_box.dimensions.margin.left, 200.0);
    assert_eq!(img_box.dimensions.margin.right, 200.0);
    assert_eq!(img_box.dimensions.content.x, 200.0);
}

#[test]
fn test_build_layout_tree_with_image_cache_and_html_attributes() {
    let doc = parse_html(r#"<html><body><img src="https://example.com/logo.png" width="120" /></body></html>"#);
    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    }).expect("html");

    let mut cache = std::collections::HashMap::new();
    let img_data = std::sync::Arc::new(crate::DecodedImage::new(240, 120, vec![0; 240 * 120 * 4]));
    cache.insert("https://example.com/logo.png".to_string(), img_data);

    let style_tree = lilies_css::compute_styles_for_document(&doc);
    let layout_tree = crate::tree_builder::build_layout_tree_with_images(
        html_id,
        doc.arena(),
        &style_tree,
        &cache,
    )
    .expect("layout tree");

    // Traversal: html -> body -> img (or anonymous block)
    let body_box = &layout_tree.children[0];
    let img_box = if body_box.children[0].is_image() {
        &body_box.children[0]
    } else {
        &body_box.children[0].children[0]
    };
    assert!(img_box.is_image());
    assert_eq!(img_box.dimensions.content.width, 120.0);
    assert_eq!(img_box.dimensions.content.height, 60.0); // 120 * (120/240) = 60.0
}

#[test]
fn test_build_layout_tree_image_max_width_preserves_aspect_ratio() {
    let doc = parse_html(
        r#"<html><body><img src="https://example.com/wide.png" style="max-width: 250px;" /></body></html>"#,
    );
    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("html");

    let mut cache = std::collections::HashMap::new();
    let img_data = std::sync::Arc::new(crate::DecodedImage::new(400, 200, vec![0; 400 * 200 * 4]));
    cache.insert("https://example.com/wide.png".to_string(), img_data);

    let style_tree = lilies_css::compute_styles_for_document(&doc);
    let mut layout_tree = crate::tree_builder::build_layout_tree_with_images(
        html_id,
        doc.arena(),
        &style_tree,
        &cache,
    )
    .expect("layout tree");

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    layout_tree.layout(cb);

    let body_box = &layout_tree.children[0];
    let img_box = if body_box.children[0].is_image() {
        &body_box.children[0]
    } else {
        &body_box.children[0].children[0]
    };
    assert!(img_box.is_image());
    assert_eq!(img_box.dimensions.content.width, 250.0);
    assert_eq!(img_box.dimensions.content.height, 125.0);
}

#[test]
fn test_anonymous_block_does_not_duplicate_parent_padding_or_margin() {
    let doc = parse_html(
        r#"<html><body><div style="padding: 24px; margin: 40px; background-color: #112233; text-align: center;"><p>Block</p><span>Inline</span></div></body></html>"#,
    );
    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("html");

    let style_tree = lilies_css::compute_styles_for_document(&doc);
    let layout_tree = crate::tree_builder::build_layout_tree(html_id, doc.arena(), &style_tree)
        .expect("layout tree");

    let body_box = &layout_tree.children[0];
    let div_box = &body_box.children[0];
    let anon_box = &div_box.children[1];
    assert!(anon_box.is_anonymous());

    let anon_style = anon_box.style.as_ref().expect("anon style");
    assert_eq!(anon_style.padding_top, 0.0);
    assert_eq!(anon_style.padding_left, 0.0);
    assert_eq!(anon_style.margin_top, LengthOrAuto::Px(0.0));
    assert_eq!(anon_style.background_color, lilies_css::Color::TRANSPARENT);
    assert_eq!(anon_style.text_align, lilies_css::TextAlign::Center);
}

#[test]
fn test_inline_siblings_flow_horizontally_and_nested_inlines_do_not_overlap() {
    let doc = parse_html(
        r##"<html><body>
            <p id="p1">Hello <b>world</b> and <a href="#"><span>A</span><span>B</span></a></p>
            <div id="narrow" style="width: 120px;"><span>One two three four five six seven eight</span></div>
        </body></html>"##,
    );
    let html_id = doc
        .get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .expect("html");

    let style_tree = lilies_css::compute_styles_for_document(&doc);
    let mut layout_tree =
        crate::tree_builder::build_layout_tree(html_id, doc.arena(), &style_tree)
            .expect("layout tree");
    layout_tree.layout(Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)));

    let body = &layout_tree.children[0];
    let p1 = &body.children[0];
    assert_eq!(p1.children.len(), 4);

    // 1. Sibling inlines in <p> flow horizontally on the same line
    let c0 = &p1.children[0];
    let c1 = &p1.children[1];
    let c2 = &p1.children[2];
    let c3 = &p1.children[3];
    assert!(c0.dimensions.content.x < c1.dimensions.content.x);
    assert!(c1.dimensions.content.x < c2.dimensions.content.x);
    assert!(c2.dimensions.content.x < c3.dimensions.content.x);
    assert_eq!(c0.dimensions.content.y, c1.dimensions.content.y);
    assert_eq!(c1.dimensions.content.y, c2.dimensions.content.y);
    assert_eq!(c2.dimensions.content.y, c3.dimensions.content.y);

    // 2. Nested <span>A</span> and <span>B</span> inside <a> do not overlap
    assert_eq!(c3.children.len(), 2);
    let span_a = &c3.children[0];
    let span_b = &c3.children[1];
    assert!(
        span_a.dimensions.content.x + span_a.dimensions.content.width
            <= span_b.dimensions.content.x + 0.01
    );

    // 3. Text inside <span> inside narrow <div> wraps across multiple lines
    let narrow_div = &body.children[1];
    let narrow_span = &narrow_div.children[0];
    let narrow_text = &narrow_span.children[0];
    assert!(narrow_text.lines.len() >= 2);
    assert!(narrow_div.dimensions.content.height > 20.0);
}

#[test]
fn test_missing_image_fallback_to_alt_text() {
    let doc = parse_html(r#"<html><body><img src="missing.png" alt="Fallback Alt" /></body></html>"#);
    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    }).expect("html");

    let cache = std::collections::HashMap::new();
    let style_tree = lilies_css::compute_styles_for_document(&doc);
    let layout_tree = crate::tree_builder::build_layout_tree_with_images(
        html_id,
        doc.arena(),
        &style_tree,
        &cache,
    )
    .expect("layout tree");

    let body_box = &layout_tree.children[0];
    let fallback = if body_box.children[0].text.is_some() {
        &body_box.children[0]
    } else {
        &body_box.children[0].children[0]
    };
    assert_eq!(fallback.text.as_deref(), Some("Fallback Alt"));
    assert!(fallback.dimensions.content.width > 0.0);
    assert!(fallback.dimensions.content.height > 0.0);
}

#[test]
fn test_flex_row_side_by_side_increasing_x_identical_y() {
    let mut container = LayoutBox::new(BoxType::AnonymousBlock);
    container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        justify_content: JustifyContent::FlexStart,
        align_items: AlignItems::FlexStart,
        ..Default::default()
    });

    for _ in 0..3 {
        let mut child = LayoutBox::new(BoxType::AnonymousBlock);
        child.style = Some(ComputedStyle {
            display: Display::Block,
            width: LengthOrAuto::Px(80.0),
            height: LengthOrAuto::Px(40.0),
            ..Default::default()
        });
        container.children.push(child);
    }

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 400.0, 0.0));
    container.layout(cb);

    let c0 = &container.children[0];
    let c1 = &container.children[1];
    let c2 = &container.children[2];

    assert_eq!(c0.dimensions.content.x, 0.0);
    assert_eq!(c1.dimensions.content.x, 80.0);
    assert_eq!(c2.dimensions.content.x, 160.0);

    // Verify children have increasing X coordinates side-by-side with identical Y coordinates
    assert!(c0.dimensions.content.x < c1.dimensions.content.x);
    assert!(c1.dimensions.content.x < c2.dimensions.content.x);
    assert_eq!(c0.dimensions.content.y, c1.dimensions.content.y);
    assert_eq!(c1.dimensions.content.y, c2.dimensions.content.y);
    assert_eq!(c0.dimensions.content.y, 0.0);
}

#[test]
fn test_flex_justify_content_space_between_and_center() {
    // 1. Center
    let mut center_container = LayoutBox::new(BoxType::AnonymousBlock);
    center_container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::FlexStart,
        ..Default::default()
    });
    for _ in 0..2 {
        let mut child = LayoutBox::new(BoxType::AnonymousBlock);
        child.style = Some(ComputedStyle {
            display: Display::Block,
            width: LengthOrAuto::Px(100.0),
            height: LengthOrAuto::Px(40.0),
            ..Default::default()
        });
        center_container.children.push(child);
    }
    let cb = Dimensions::new(Rect::new(0.0, 0.0, 400.0, 0.0));
    center_container.layout(cb);

    // Total children width = 200. Free space = 200. Left space = 100.
    assert_eq!(center_container.children[0].dimensions.content.x, 100.0);
    assert_eq!(center_container.children[1].dimensions.content.x, 200.0);

    // 2. SpaceBetween
    let mut sb_container = LayoutBox::new(BoxType::AnonymousBlock);
    sb_container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::FlexStart,
        ..Default::default()
    });
    for _ in 0..2 {
        let mut child = LayoutBox::new(BoxType::AnonymousBlock);
        child.style = Some(ComputedStyle {
            display: Display::Block,
            width: LengthOrAuto::Px(100.0),
            height: LengthOrAuto::Px(40.0),
            ..Default::default()
        });
        sb_container.children.push(child);
    }
    sb_container.layout(cb);

    // First item at start (0), last item at end (400 - 100 = 300)
    assert_eq!(sb_container.children[0].dimensions.content.x, 0.0);
    assert_eq!(sb_container.children[1].dimensions.content.x, 300.0);

    // 3. SpaceAround
    let mut sa_container = LayoutBox::new(BoxType::AnonymousBlock);
    sa_container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        justify_content: JustifyContent::SpaceAround,
        align_items: AlignItems::FlexStart,
        ..Default::default()
    });
    for _ in 0..2 {
        let mut child = LayoutBox::new(BoxType::AnonymousBlock);
        child.style = Some(ComputedStyle {
            display: Display::Block,
            width: LengthOrAuto::Px(100.0),
            height: LengthOrAuto::Px(40.0),
            ..Default::default()
        });
        sa_container.children.push(child);
    }
    sa_container.layout(cb);

    // Free space = 200. Per item = 100. Item 0 starts at 100 / 2 = 50. Item 1 starts at 50 + 100 + 100 = 250.
    assert_eq!(sa_container.children[0].dimensions.content.x, 50.0);
    assert_eq!(sa_container.children[1].dimensions.content.x, 250.0);
}

#[test]
fn test_flex_grow_expansion_of_free_space() {
    let mut container = LayoutBox::new(BoxType::AnonymousBlock);
    container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        justify_content: JustifyContent::FlexStart,
        ..Default::default()
    });

    let mut c1 = LayoutBox::new(BoxType::AnonymousBlock);
    c1.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(100.0),
        height: LengthOrAuto::Px(50.0),
        flex_grow: 1.0,
        ..Default::default()
    });

    let mut c2 = LayoutBox::new(BoxType::AnonymousBlock);
    c2.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(100.0),
        height: LengthOrAuto::Px(50.0),
        flex_grow: 3.0,
        ..Default::default()
    });

    container.children.push(c1);
    container.children.push(c2);

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 400.0, 0.0));
    container.layout(cb);

    // Total base = 200. Free = 200. Total flex grow = 4.
    // c1 gets 200 * (1/4) = 50 -> width 150.
    // c2 gets 200 * (3/4) = 150 -> width 250.
    assert_eq!(container.children[0].dimensions.content.width, 150.0);
    assert_eq!(container.children[1].dimensions.content.width, 250.0);
    assert_eq!(container.children[0].dimensions.content.x, 0.0);
    assert_eq!(container.children[1].dimensions.content.x, 150.0);
}

#[test]
fn test_flex_gap_offset_between_items() {
    let mut container = LayoutBox::new(BoxType::AnonymousBlock);
    container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        width: LengthOrAuto::Px(400.0),
        height: LengthOrAuto::Px(100.0),
        gap: 24.0,
        justify_content: JustifyContent::FlexStart,
        ..Default::default()
    });

    for _ in 0..3 {
        let mut child = LayoutBox::new(BoxType::AnonymousBlock);
        child.style = Some(ComputedStyle {
            display: Display::Block,
            width: LengthOrAuto::Px(80.0),
            height: LengthOrAuto::Px(40.0),
            ..Default::default()
        });
        container.children.push(child);
    }

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 400.0, 0.0));
    container.layout(cb);

    // Item 0 at 0.
    // Item 1 at 80 + 24 = 104.
    // Item 2 at 104 + 80 + 24 = 208.
    assert_eq!(container.children[0].dimensions.content.x, 0.0);
    assert_eq!(container.children[1].dimensions.content.x, 104.0);
    assert_eq!(container.children[2].dimensions.content.x, 208.0);
}

#[test]
fn test_flex_column_layout_and_align_items_center() {
    let mut container = LayoutBox::new(BoxType::AnonymousBlock);
    container.style = Some(ComputedStyle {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        width: LengthOrAuto::Px(300.0),
        height: LengthOrAuto::Px(300.0),
        gap: 10.0,
        justify_content: JustifyContent::FlexStart,
        align_items: AlignItems::Center,
        ..Default::default()
    });

    let mut c1 = LayoutBox::new(BoxType::AnonymousBlock);
    c1.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(100.0),
        height: LengthOrAuto::Px(50.0),
        ..Default::default()
    });

    let mut c2 = LayoutBox::new(BoxType::AnonymousBlock);
    c2.style = Some(ComputedStyle {
        display: Display::Block,
        width: LengthOrAuto::Px(160.0),
        height: LengthOrAuto::Px(60.0),
        ..Default::default()
    });

    container.children.push(c1);
    container.children.push(c2);

    let cb = Dimensions::new(Rect::new(0.0, 0.0, 300.0, 0.0));
    container.layout(cb);

    // c1: centered horizontally: (300 - 100) / 2 = 100. y = 0.
    assert_eq!(container.children[0].dimensions.content.x, 100.0);
    assert_eq!(container.children[0].dimensions.content.y, 0.0);

    // c2: centered horizontally: (300 - 160) / 2 = 70. y = 50 + 10 = 60.
    assert_eq!(container.children[1].dimensions.content.x, 70.0);
    assert_eq!(container.children[1].dimensions.content.y, 60.0);
}

#[test]
fn test_flexbox_demo_html_tree_layout() {
    let html = r#"<html>
      <body style="padding: 20px;">
        <nav style="display: flex; justify-content: space-between; align-items: center; padding: 16px 24px; margin-bottom: 24px;">
          <h2 style="font-size: 20px; margin: 0;">lilies</h2>
          <div style="display: flex; gap: 16px;">
            <span>Home</span>
            <span>Docs</span>
            <span>GitHub</span>
          </div>
        </nav>
        <div style="display: flex; justify-content: space-between; gap: 20px;">
          <div style="flex-grow: 1; padding: 20px;">
            <h3>Card One</h3>
            <p>First flex item.</p>
          </div>
          <div style="flex-grow: 1; padding: 20px;">
            <h3>Card Two</h3>
            <p>Second flex item.</p>
          </div>
        </div>
      </body>
    </html>"#;

    let doc = parse_html(html);
    let styles = lilies_css::compute_styles_for_document(&doc);
    let html_id = doc.get_children(doc.root()).into_iter().find(|&id| {
        doc.node(id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.tag_name() == "html")
    }).expect("html");

    let mut layout_tree = build_layout_tree(html_id, doc.arena(), &styles).expect("layout tree");
    crate::layout(&mut layout_tree, Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)));

    let body = &layout_tree.children[0];
    let nav = &body.children[0];
    assert_eq!(nav.children.len(), 2);

    let h2 = &nav.children[0];
    let nav_links = &nav.children[1];

    // h2 at left, links at right (space-between)
    assert!(h2.dimensions.content.x < nav_links.dimensions.content.x);
    assert_eq!(nav_links.children.len(), 3);
    assert!(nav_links.children[0].dimensions.content.x < nav_links.children[1].dimensions.content.x);
    assert!(nav_links.children[1].dimensions.content.x < nav_links.children[2].dimensions.content.x);

    let cards_grid = &body.children[1];
    assert_eq!(cards_grid.children.len(), 2);
    let card1 = &cards_grid.children[0];
    let card2 = &cards_grid.children[1];

    // Both cards should have positive grown width, side-by-side
    assert!(card1.dimensions.content.width > 200.0);
    assert!(card2.dimensions.content.width > 200.0);
    assert!(card1.dimensions.content.x < card2.dimensions.content.x);
    assert_eq!(card1.dimensions.content.y, card2.dimensions.content.y);
}
