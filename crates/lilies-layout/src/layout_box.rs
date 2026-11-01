#![deny(unsafe_code)]

use crate::box_model::Dimensions;
use crate::font::TextLine;
use lilies_css::{ComputedStyle, LengthOrAuto};
use lilies_dom::NodeId;

/// Represents the formatting category of a layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    BlockNode(NodeId),
    InlineNode(NodeId),
    AnonymousBlock,
}

impl BoxType {
    /// Returns true if this box is block-level.
    pub fn is_block(&self) -> bool {
        matches!(self, BoxType::BlockNode(_) | BoxType::AnonymousBlock)
    }

    /// Returns true if this box is inline-level.
    pub fn is_inline(&self) -> bool {
        matches!(self, BoxType::InlineNode(_))
    }

    /// Returns true if this box is an anonymous block box.
    pub fn is_anonymous(&self) -> bool {
        matches!(self, BoxType::AnonymousBlock)
    }

    /// Returns the associated DOM `NodeId` if this box is not anonymous.
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            BoxType::BlockNode(id) | BoxType::InlineNode(id) => Some(*id),
            BoxType::AnonymousBlock => None,
        }
    }
}

/// A node within the layout tree with physical dimensions and hierarchy.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    pub dimensions: Dimensions,
    pub box_type: BoxType,
    pub children: Vec<LayoutBox>,
    pub style: Option<ComputedStyle>,
    pub text: Option<String>,
    pub lines: Vec<TextLine>,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            dimensions: Dimensions::default(),
            box_type: BoxType::AnonymousBlock,
            children: Vec::new(),
            style: None,
            text: None,
            lines: Vec::new(),
        }
    }
}

impl LayoutBox {
    /// Creates a new layout box of the given type with default dimensions and no children.
    pub fn new(box_type: BoxType) -> Self {
        Self {
            dimensions: Dimensions::default(),
            box_type,
            children: Vec::new(),
            style: None,
            text: None,
            lines: Vec::new(),
        }
    }

    /// Sets the text content for this layout box.
    pub fn with_text(mut self, text: String) -> Self {
        self.text = Some(text);
        self
    }

    /// Sets the computed style for this layout box.
    pub fn with_style(mut self, style: ComputedStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Returns a reference to the computed style, if present.
    pub fn get_style(&self) -> Option<&ComputedStyle> {
        self.style.as_ref()
    }

    /// Returns true if this box is block-level.
    pub fn is_block(&self) -> bool {
        self.box_type.is_block()
    }

    /// Returns true if this box is inline-level.
    pub fn is_inline(&self) -> bool {
        self.box_type.is_inline()
    }

    /// Returns true if this box is an anonymous block box.
    pub fn is_anonymous(&self) -> bool {
        self.box_type.is_anonymous()
    }

    /// Returns the associated DOM `NodeId`, if any.
    pub fn node_id(&self) -> Option<NodeId> {
        self.box_type.node_id()
    }

    /// Performs hit-testing to find the innermost DOM `NodeId` under coordinate `(x, y)`.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        crate::hit_test::hit_test(self, x, y)
    }

    /// Recursively lays out this box and all of its descendants within the containing block.
    pub fn layout(&mut self, containing_block: Dimensions) {
        match self.box_type {
            BoxType::BlockNode(_) | BoxType::AnonymousBlock => self.layout_block(containing_block),
            BoxType::InlineNode(_) => self.layout_inline(containing_block),
        }
    }

    /// Lays out a block-level box according to CSS 2.1 Block Formatting Context rules.
    pub fn layout_block(&mut self, containing_block: Dimensions) {
        self.calculate_block_width(&containing_block);
        self.calculate_block_position(&containing_block);
        self.layout_block_children();
        self.calculate_block_height();
    }

    /// Calculates horizontal margins, padding, borders, and content width for a block box.
    ///
    /// According to CSS 2.1 Section 10.3.3:
    /// `margin_left + border_left + padding_left + width + padding_right + border_right + margin_right`
    /// must equal `containing_block.content.width`.
    pub fn calculate_block_width(&mut self, containing_block: &Dimensions) {
        let (width, margin_left, margin_right, padding_left, padding_right) =
            if let Some(ref style) = self.style {
                (
                    style.width,
                    style.margin_left,
                    style.margin_right,
                    style.padding_left,
                    style.padding_right,
                )
            } else {
                (
                    if self.dimensions.content.width > 0.0 {
                        LengthOrAuto::Px(self.dimensions.content.width)
                    } else {
                        LengthOrAuto::Auto
                    },
                    LengthOrAuto::Px(self.dimensions.margin.left),
                    LengthOrAuto::Px(self.dimensions.margin.right),
                    self.dimensions.padding.left,
                    self.dimensions.padding.right,
                )
            };

        if let Some(ref style) = self.style {
            self.dimensions.padding.top = style.padding_top;
            self.dimensions.padding.bottom = style.padding_bottom;
            self.dimensions.padding.left = style.padding_left;
            self.dimensions.padding.right = style.padding_right;

            self.dimensions.margin.top = match style.margin_top {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };
            self.dimensions.margin.bottom = match style.margin_bottom {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };
        }

        let cb_width = containing_block.content.width;
        let border_left = self.dimensions.border.left;
        let border_right = self.dimensions.border.right;

        let w_px = match width {
            LengthOrAuto::Px(px) => px,
            LengthOrAuto::Auto => 0.0,
        };
        let mut ml_px = match margin_left {
            LengthOrAuto::Px(px) => px,
            LengthOrAuto::Auto => 0.0,
        };
        let mut mr_px = match margin_right {
            LengthOrAuto::Px(px) => px,
            LengthOrAuto::Auto => 0.0,
        };

        let is_width_auto = width.is_auto();
        let mut is_ml_auto = margin_left.is_auto();
        let mut is_mr_auto = margin_right.is_auto();

        let total =
            ml_px + border_left + padding_left + w_px + padding_right + border_right + mr_px;

        // If width is not auto and the total is bigger than the container, treat auto margins as 0.
        if !is_width_auto && total > cb_width {
            if is_ml_auto {
                is_ml_auto = false;
                ml_px = 0.0;
            }
            if is_mr_auto {
                is_mr_auto = false;
                mr_px = 0.0;
            }
        }

        let underflow = cb_width
            - (ml_px + border_left + padding_left + w_px + padding_right + border_right + mr_px);

        match (is_width_auto, is_ml_auto, is_mr_auto) {
            // If width is auto, any auto margins become 0, and width absorbs remaining space.
            (true, _, _) => {
                let ml = if is_ml_auto { 0.0 } else { ml_px };
                let mr = if is_mr_auto { 0.0 } else { mr_px };
                self.dimensions.margin.left = ml;

                let other = ml + border_left + padding_left + padding_right + border_right + mr;
                let available = cb_width - other;
                if available >= 0.0 {
                    self.dimensions.content.width = available;
                    self.dimensions.margin.right = mr;
                } else {
                    // Width cannot be negative; adjust margin_right to absorb negative underflow.
                    self.dimensions.content.width = 0.0;
                    self.dimensions.margin.right = mr + available;
                }
            }

            // Fixed width and both margins auto: center horizontally.
            (false, true, true) => {
                let half = underflow / 2.0;
                self.dimensions.content.width = w_px;
                self.dimensions.margin.left = half;
                self.dimensions.margin.right = half;
            }

            // Fixed width and auto margin-left:
            (false, true, false) => {
                self.dimensions.content.width = w_px;
                self.dimensions.margin.left = underflow;
                self.dimensions.margin.right = mr_px;
            }

            // Fixed width and auto margin-right:
            (false, false, true) => {
                self.dimensions.content.width = w_px;
                self.dimensions.margin.left = ml_px;
                self.dimensions.margin.right = underflow;
            }

            // Overconstrained: adjust margin_right to absorb underflow.
            (false, false, false) => {
                self.dimensions.content.width = w_px;
                self.dimensions.margin.left = ml_px;
                self.dimensions.margin.right = mr_px + underflow;
            }
        }
    }

    /// Calculates the (x, y) coordinates of the content box relative to the containing block.
    pub fn calculate_block_position(&mut self, containing_block: &Dimensions) {
        self.dimensions.content.x = containing_block.content.x
            + self.dimensions.margin.left
            + self.dimensions.border.left
            + self.dimensions.padding.left;

        self.dimensions.content.y = containing_block.content.y
            + containing_block.content.height
            + self.dimensions.margin.top
            + self.dimensions.border.top
            + self.dimensions.padding.top;
    }

    /// Recursively lays out all children, accumulating their margin boxes into content height.
    pub fn layout_block_children(&mut self) {
        self.dimensions.content.height = 0.0;

        for child in &mut self.children {
            child.layout(self.dimensions);
            self.dimensions.content.height += child.dimensions.margin_box().height;
        }
    }

    /// Sets explicit height if specified, or retains the accumulated children's height if auto.
    pub fn calculate_block_height(&mut self) {
        let explicit_height = if let Some(ref style) = self.style {
            style.height.to_px()
        } else if self.children.is_empty() && self.dimensions.content.height > 0.0 {
            Some(self.dimensions.content.height)
        } else {
            None
        };

        if let Some(h) = explicit_height {
            self.dimensions.content.height = h;
        }
    }

    /// Basic layout for an inline-level box.
    pub fn layout_inline(&mut self, containing_block: Dimensions) {
        if let Some(ref style) = self.style {
            self.dimensions.padding.top = style.padding_top;
            self.dimensions.padding.bottom = style.padding_bottom;
            self.dimensions.padding.left = style.padding_left;
            self.dimensions.padding.right = style.padding_right;

            self.dimensions.margin.top = match style.margin_top {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };
            self.dimensions.margin.bottom = match style.margin_bottom {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };
            self.dimensions.margin.left = match style.margin_left {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };
            self.dimensions.margin.right = match style.margin_right {
                LengthOrAuto::Px(v) => v,
                LengthOrAuto::Auto => 0.0,
            };

            if let Some(w) = style.width.to_px() {
                self.dimensions.content.width = w;
            }
            if let Some(h) = style.height.to_px() {
                self.dimensions.content.height = h;
            }
        }

        self.dimensions.content.x = containing_block.content.x
            + self.dimensions.margin.left
            + self.dimensions.border.left
            + self.dimensions.padding.left;

        self.dimensions.content.y = containing_block.content.y
            + containing_block.content.height
            + self.dimensions.margin.top
            + self.dimensions.border.top
            + self.dimensions.padding.top;

        if let Some(ref text) = self.text {
            let font_size = self.style.as_ref().map_or(16.0, |s| s.font_size);
            let available_width = if self.dimensions.content.width > 0.0 {
                self.dimensions.content.width
            } else {
                containing_block.content.width
            };
            self.lines = crate::font::wrap_text(
                text,
                font_size,
                available_width,
                self.dimensions.content.x,
                self.dimensions.content.y,
            );
            if !self.lines.is_empty() {
                let line_h = crate::font::line_height(font_size);
                self.dimensions.content.height = self.lines.len() as f32 * line_h;
                let max_w = self
                    .lines
                    .iter()
                    .map(|l| l.rect.width)
                    .fold(0.0f32, f32::max);
                self.dimensions.content.width = max_w;
            }
        }

        if !self.children.is_empty() {
            let mut total_h = 0.0f32;
            let mut max_w = 0.0f32;
            let mut child_cb = self.dimensions;
            if child_cb.content.width == 0.0 {
                child_cb.content.width = containing_block.content.width;
            }
            for child in &mut self.children {
                child.layout(child_cb);
                total_h += child.dimensions.margin_box().height;
                max_w = max_w.max(child.dimensions.margin_box().width);
            }
            if self.dimensions.content.height == 0.0 {
                self.dimensions.content.height = total_h;
            }
            if self.dimensions.content.width == 0.0 {
                self.dimensions.content.width = max_w;
            }
        }
    }
}
