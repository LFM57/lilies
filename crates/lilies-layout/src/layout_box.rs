#![deny(unsafe_code)]

use crate::box_model::Dimensions;
use crate::font::TextLine;
use lilies_css::{
    AlignItems, ComputedStyle, Display, FlexDirection, JustifyContent, LengthOrAuto,
};
use lilies_dom::NodeId;
use lilies_net::DecodedImage;
use std::sync::Arc;

/// Represents the formatting category of a layout box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxType {
    BlockNode(NodeId),
    InlineNode(NodeId),
    AnonymousBlock,
    ImageNode(NodeId, Arc<DecodedImage>),
}

impl BoxType {
    /// Returns true if this box is block-level.
    pub fn is_block(&self) -> bool {
        matches!(self, BoxType::BlockNode(_) | BoxType::AnonymousBlock)
    }

    /// Returns true if this box is inline-level.
    pub fn is_inline(&self) -> bool {
        matches!(self, BoxType::InlineNode(_) | BoxType::ImageNode(..))
    }

    /// Returns true if this box is an anonymous block box.
    pub fn is_anonymous(&self) -> bool {
        matches!(self, BoxType::AnonymousBlock)
    }

    /// Returns true if this box is an image node.
    pub fn is_image(&self) -> bool {
        matches!(self, BoxType::ImageNode(..))
    }

    /// Returns the associated DOM `NodeId` if this box is not anonymous.
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            BoxType::BlockNode(id) | BoxType::InlineNode(id) | BoxType::ImageNode(id, _) => {
                Some(*id)
            }
            BoxType::AnonymousBlock => None,
        }
    }

    /// Returns the associated `DecodedImage` if this box is an image node.
    pub fn image(&self) -> Option<&Arc<DecodedImage>> {
        match self {
            BoxType::ImageNode(_, img) => Some(img),
            _ => None,
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
        if let BoxType::ImageNode(..) = self.box_type {
            self.style
                .as_ref()
                .is_some_and(|s| s.display == lilies_css::Display::Block)
        } else {
            self.box_type.is_block()
        }
    }

    /// Returns true if this box is inline-level.
    pub fn is_inline(&self) -> bool {
        if let BoxType::ImageNode(..) = self.box_type {
            self.style
                .as_ref()
                .is_none_or(|s| s.display != lilies_css::Display::Block)
        } else {
            self.box_type.is_inline()
        }
    }

    /// Returns true if this box is an anonymous block box.
    pub fn is_anonymous(&self) -> bool {
        self.box_type.is_anonymous()
    }

    /// Returns true if this box is an image box.
    pub fn is_image(&self) -> bool {
        self.box_type.is_image()
    }

    /// Returns the associated image if this is an image node.
    pub fn image(&self) -> Option<&Arc<DecodedImage>> {
        self.box_type.image()
    }

    /// Returns the associated DOM `NodeId`, if any.
    pub fn node_id(&self) -> Option<NodeId> {
        self.box_type.node_id()
    }

    /// Performs hit-testing to find the innermost DOM `NodeId` under coordinate `(x, y)`.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        crate::hit_test::hit_test(self, x, y)
    }

    /// Recursively translates this layout box and all descendants / text lines by (dx, dy).
    pub fn offset_by(&mut self, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        self.dimensions.content.x += dx;
        self.dimensions.content.y += dy;
        for line in &mut self.lines {
            line.rect.x += dx;
            line.rect.y += dy;
        }
        for child in &mut self.children {
            child.offset_by(dx, dy);
        }
    }

    /// Recursively lays out this box and all of its descendants within the containing block.
    pub fn layout(&mut self, containing_block: Dimensions) {
        if self
            .style
            .as_ref()
            .is_some_and(|s| s.display == Display::Flex)
        {
            self.layout_flex(&containing_block);
            return;
        }
        match &self.box_type {
            BoxType::BlockNode(_) | BoxType::AnonymousBlock => self.layout_block(containing_block),
            BoxType::InlineNode(_) => self.layout_inline(containing_block),
            BoxType::ImageNode(..) => self.layout_image(containing_block),
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
        let is_ml_auto_orig = margin_left.is_auto();
        let is_mr_auto_orig = margin_right.is_auto();
        let mut is_ml_auto = is_ml_auto_orig;
        let mut is_mr_auto = is_mr_auto_orig;

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

        let max_width = self.style.as_ref().and_then(|s| s.max_width);
        if let Some(max_w) = max_width {
            if self.dimensions.content.width > max_w {
                self.dimensions.content.width = max_w;

                // Recalculate horizontal auto-margins using the clamped width
                let non_content_h = border_left + padding_left + padding_right + border_right;
                let available_for_margins = cb_width - (max_w + non_content_h);

                match (is_ml_auto_orig, is_mr_auto_orig) {
                    (true, true) => {
                        let half = (available_for_margins / 2.0).max(0.0);
                        self.dimensions.margin.left = half;
                        self.dimensions.margin.right = half;
                    }
                    (true, false) => {
                        self.dimensions.margin.right = mr_px;
                        self.dimensions.margin.left = (available_for_margins - mr_px).max(0.0);
                    }
                    (false, true) => {
                        self.dimensions.margin.left = ml_px;
                        self.dimensions.margin.right = (available_for_margins - ml_px).max(0.0);
                    }
                    (false, false) => {
                        self.dimensions.margin.left = ml_px;
                        self.dimensions.margin.right = mr_px + (available_for_margins - ml_px - mr_px);
                    }
                }
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
                let text_align = self
                    .style
                    .as_ref()
                    .map_or(lilies_css::TextAlign::Left, |s| s.text_align);
                let containing_block_width = available_width;
                if text_align != lilies_css::TextAlign::Left && containing_block_width.is_finite() {
                    for line in &mut self.lines {
                        let space = containing_block_width - line.rect.width;
                        match text_align {
                            lilies_css::TextAlign::Center => {
                                line.rect.x += space / 2.0;
                            }
                            lilies_css::TextAlign::Right => {
                                line.rect.x += space;
                            }
                            lilies_css::TextAlign::Left => {}
                        }
                    }
                }
                let line_h = crate::font::line_height(font_size);
                self.dimensions.content.height = self.lines.len() as f32 * line_h;
                let min_x = self
                    .lines
                    .iter()
                    .map(|l| l.rect.x)
                    .fold(f32::INFINITY, f32::min);
                let max_x = self
                    .lines
                    .iter()
                    .map(|l| l.rect.x + l.rect.width)
                    .fold(f32::NEG_INFINITY, f32::max);
                if min_x.is_finite() && max_x.is_finite() && max_x >= min_x {
                    self.dimensions.content.x = min_x;
                    self.dimensions.content.width = max_x - min_x;
                }
            }
        }

        if !self.children.is_empty() {
            let mut total_h = 0.0f32;
            let mut max_w = 0.0f32;
            let mut child_cb = self.dimensions;
            if child_cb.content.width == 0.0 {
                child_cb.content.width = f32::INFINITY;
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

            // Align this inline container within containing_block if text-align is specified
            let text_align = self
                .style
                .as_ref()
                .map_or(lilies_css::TextAlign::Left, |s| s.text_align);
            let total_w = self.dimensions.margin_box().width;
            let space = containing_block.content.width - total_w;
            if space > 0.0 && containing_block.content.width.is_finite() {
                let shift_x = match text_align {
                    lilies_css::TextAlign::Center => space / 2.0,
                    lilies_css::TextAlign::Right => space,
                    lilies_css::TextAlign::Left => 0.0,
                };
                if shift_x > 0.0 {
                    self.offset_by(shift_x, 0.0);
                }
            }
        }
    }

    /// Lays out a replaced image box according to intrinsic dimensions, aspect ratio,
    /// and explicit CSS or HTML width/height constraints.
    pub fn layout_image(&mut self, containing_block: Dimensions) {
        let image = match &self.box_type {
            BoxType::ImageNode(_, img) => Arc::clone(img),
            _ => return,
        };

        let intrinsic_w = image.width as f32;
        let intrinsic_h = image.height as f32;

        let (width, margin_left, margin_right, padding_left, padding_right) =
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

                (
                    style.width,
                    style.margin_left,
                    style.margin_right,
                    style.padding_left,
                    style.padding_right,
                )
            } else {
                (
                    LengthOrAuto::Px(self.dimensions.content.width),
                    LengthOrAuto::Px(self.dimensions.margin.left),
                    LengthOrAuto::Px(self.dimensions.margin.right),
                    self.dimensions.padding.left,
                    self.dimensions.padding.right,
                )
            };

        let height = self.style.as_ref().map_or(LengthOrAuto::Auto, |s| s.height);

        // Aspect ratio calculations:
        // - Both specified: use both.
        // - Only width: height = width * (intrinsic_h / intrinsic_w).
        // - Only height: width = height * (intrinsic_w / intrinsic_h).
        // - Neither: use intrinsic dimensions.
        let (mut w_px, mut h_px) = match (width.to_px(), height.to_px()) {
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
            (None, None) => {
                if self.dimensions.content.width > 0.0 && self.dimensions.content.height > 0.0 {
                    (self.dimensions.content.width, self.dimensions.content.height)
                } else {
                    (intrinsic_w, intrinsic_h)
                }
            }
        };

        // Max-width constraint clamping while preserving aspect ratio
        if let Some(max_w) = self.style.as_ref().and_then(|s| s.max_width) {
            if w_px > max_w {
                let ratio = if w_px > 0.0 { max_w / w_px } else { 1.0 };
                w_px = max_w;
                if height.is_auto() {
                    h_px *= ratio;
                }
            }
        }

        self.dimensions.content.width = w_px;
        self.dimensions.content.height = h_px;

        let border_left = self.dimensions.border.left;
        let border_right = self.dimensions.border.right;
        let cb_width = containing_block.content.width;

        let is_ml_auto = margin_left.is_auto();
        let is_mr_auto = margin_right.is_auto();
        let ml_px = match margin_left {
            LengthOrAuto::Px(px) => px,
            LengthOrAuto::Auto => 0.0,
        };
        let mr_px = match margin_right {
            LengthOrAuto::Px(px) => px,
            LengthOrAuto::Auto => 0.0,
        };

        let total_non_margin = border_left + padding_left + w_px + padding_right + border_right;
        let available_for_margins = (cb_width - total_non_margin).max(0.0);

        match (is_ml_auto, is_mr_auto) {
            (true, true) => {
                let half = available_for_margins / 2.0;
                self.dimensions.margin.left = half;
                self.dimensions.margin.right = half;
            }
            (true, false) => {
                self.dimensions.margin.left = (cb_width - total_non_margin - mr_px).max(0.0);
                self.dimensions.margin.right = mr_px;
            }
            (false, true) => {
                self.dimensions.margin.left = ml_px;
                self.dimensions.margin.right = (cb_width - total_non_margin - ml_px).max(0.0);
            }
            (false, false) => {
                self.dimensions.margin.left = ml_px;
                self.dimensions.margin.right = mr_px;

                let text_align = self
                    .style
                    .as_ref()
                    .map_or(lilies_css::TextAlign::Left, |s| s.text_align);
                let space = cb_width - (total_non_margin + ml_px + mr_px);
                if space > 0.0 {
                    match text_align {
                        lilies_css::TextAlign::Center => {
                            self.dimensions.margin.left += space / 2.0;
                        }
                        lilies_css::TextAlign::Right => {
                            self.dimensions.margin.left += space;
                        }
                        lilies_css::TextAlign::Left => {}
                    }
                }
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
    }

    /// Lays out the internal contents (text lines or descendant children) of a flex item.
    fn layout_flex_item(&mut self) {
        if self
            .style
            .as_ref()
            .is_some_and(|s| s.display == Display::Flex)
        {
            let cb = self.dimensions;
            self.layout_flex(&cb);
            return;
        }

        match &self.box_type {
            BoxType::BlockNode(_) | BoxType::AnonymousBlock => {
                self.layout_block_children();
                self.calculate_block_height();
            }
            BoxType::InlineNode(_) => {
                if let Some(ref text) = self.text {
                    let font_size = self.style.as_ref().map_or(16.0, |s| s.font_size);
                    let available_width = if self.dimensions.content.width > 0.0 {
                        self.dimensions.content.width
                    } else {
                        f32::INFINITY
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
                        let min_x = self
                            .lines
                            .iter()
                            .map(|l| l.rect.x)
                            .fold(f32::INFINITY, f32::min);
                        let max_x = self
                            .lines
                            .iter()
                            .map(|l| l.rect.x + l.rect.width)
                            .fold(f32::NEG_INFINITY, f32::max);
                        if min_x.is_finite()
                            && max_x.is_finite()
                            && max_x >= min_x
                            && self.dimensions.content.width == 0.0
                        {
                            self.dimensions.content.width = max_x - min_x;
                        }
                    }
                } else if !self.children.is_empty() {
                    let mut total_h = 0.0f32;
                    let mut max_w = 0.0f32;
                    let child_cb = self.dimensions;
                    for c in &mut self.children {
                        c.layout(child_cb);
                        total_h += c.dimensions.margin_box().height;
                        max_w = max_w.max(c.dimensions.margin_box().width);
                    }
                    if self.dimensions.content.height == 0.0 {
                        self.dimensions.content.height = total_h;
                    }
                    if self.dimensions.content.width == 0.0 {
                        self.dimensions.content.width = max_w;
                    }
                }
            }
            BoxType::ImageNode(..) => {}
        }
    }

    /// Lays out a flex container and its children according to CSS Flexbox rules.
    pub fn layout_flex(&mut self, containing_block: &Dimensions) {
        let dir = self
            .style
            .as_ref()
            .map_or(FlexDirection::Row, |s| s.flex_direction);
        match dir {
            FlexDirection::Row => self.layout_flex_row(containing_block),
            FlexDirection::Column => self.layout_flex_column(containing_block),
        }
    }

    /// Lays out items along a horizontal main axis (X) and vertical cross axis (Y).
    fn layout_flex_row(&mut self, containing_block: &Dimensions) {
        if self.dimensions.content.width == 0.0 {
            self.calculate_block_width(containing_block);
            self.calculate_block_position(containing_block);
        } else {
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
            }
            if self.dimensions.content.x == 0.0 && self.dimensions.content.y == 0.0 {
                self.calculate_block_position(containing_block);
            }
        }

        let gap = self.style.as_ref().map_or(0.0, |s| s.gap);
        let justify_content = self
            .style
            .as_ref()
            .map_or(JustifyContent::FlexStart, |s| s.justify_content);
        let align_items = self
            .style
            .as_ref()
            .map_or(AlignItems::Stretch, |s| s.align_items);

        let n = self.children.len();
        let total_gap = if n > 1 { (n - 1) as f32 * gap } else { 0.0 };

        // 1. Measure base content width for each child
        for child in &mut self.children {
            if let Some(ref style) = child.style {
                child.dimensions.padding.top = style.padding_top;
                child.dimensions.padding.bottom = style.padding_bottom;
                child.dimensions.padding.left = style.padding_left;
                child.dimensions.padding.right = style.padding_right;

                child.dimensions.margin.top = match style.margin_top {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.bottom = match style.margin_bottom {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.left = match style.margin_left {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.right = match style.margin_right {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
            }

            let base_w = if let Some(w) = child.style.as_ref().and_then(|s| s.width.to_px()) {
                w
            } else {
                measure_box_content_width(child)
            };
            child.dimensions.content.width = base_w;
        }

        let total_base_children_w: f32 = self
            .children
            .iter()
            .map(|c| {
                c.dimensions.content.width
                    + c.dimensions.margin.left
                    + c.dimensions.border.left
                    + c.dimensions.padding.left
                    + c.dimensions.padding.right
                    + c.dimensions.border.right
                    + c.dimensions.margin.right
            })
            .sum();
        let total_used_w = total_base_children_w + total_gap;

        if self.dimensions.content.width == 0.0 {
            self.dimensions.content.width = total_used_w;
        }
        let container_w = self.dimensions.content.width;

        // 2. Handle flex-grow along main axis
        let free_space = container_w - total_used_w;
        let total_flex_grow: f32 = self
            .children
            .iter()
            .map(|c| c.style.as_ref().map_or(0.0, |s| s.flex_grow))
            .sum();

        if free_space > 0.0 && total_flex_grow > 0.0 {
            for child in &mut self.children {
                let grow = child.style.as_ref().map_or(0.0, |s| s.flex_grow);
                if grow > 0.0 {
                    let added = free_space * (grow / total_flex_grow);
                    child.dimensions.content.width += added;
                }
            }
        }

        // 3. Layout each child to calculate cross-axis size (height)
        for child in &mut self.children {
            child.dimensions.content.x = self.dimensions.content.x;
            child.dimensions.content.y = self.dimensions.content.y;
            child.layout_flex_item();
        }

        // 4. Container cross-axis sizing (height)
        let explicit_height = self.style.as_ref().and_then(|s| s.height.to_px());
        let max_child_outer_h = self
            .children
            .iter()
            .map(|c| c.dimensions.margin_box().height)
            .fold(0.0f32, f32::max);
        let inner_h = explicit_height.unwrap_or(max_child_outer_h);
        self.dimensions.content.height = inner_h;

        // 5. Position items along main axis (justify-content) and cross axis (align-items)
        let final_items_w: f32 = self
            .children
            .iter()
            .map(|c| c.dimensions.margin_box().width)
            .sum::<f32>()
            + total_gap;
        let remaining_space = (container_w - final_items_w).max(0.0);

        let (mut current_x, extra_spacing) = match justify_content {
            JustifyContent::FlexStart => (self.dimensions.content.x, 0.0),
            JustifyContent::FlexEnd => (self.dimensions.content.x + remaining_space, 0.0),
            JustifyContent::Center => (self.dimensions.content.x + remaining_space / 2.0, 0.0),
            JustifyContent::SpaceBetween => {
                if n > 1 {
                    (self.dimensions.content.x, remaining_space / (n - 1) as f32)
                } else {
                    (self.dimensions.content.x, 0.0)
                }
            }
            JustifyContent::SpaceAround => {
                if n > 0 {
                    let per_item = remaining_space / n as f32;
                    (self.dimensions.content.x + per_item / 2.0, per_item)
                } else {
                    (self.dimensions.content.x, 0.0)
                }
            }
        };

        for child in &mut self.children {
            let target_x = current_x
                + child.dimensions.margin.left
                + child.dimensions.border.left
                + child.dimensions.padding.left;

            let non_content_h = child.dimensions.margin.top
                + child.dimensions.border.top
                + child.dimensions.padding.top
                + child.dimensions.padding.bottom
                + child.dimensions.border.bottom
                + child.dimensions.margin.bottom;

            let target_y = match align_items {
                AlignItems::FlexStart => {
                    self.dimensions.content.y
                        + child.dimensions.margin.top
                        + child.dimensions.border.top
                        + child.dimensions.padding.top
                }
                AlignItems::FlexEnd => {
                    let outer_h = child.dimensions.margin_box().height;
                    self.dimensions.content.y + (self.dimensions.content.height - outer_h)
                        + child.dimensions.margin.top
                        + child.dimensions.border.top
                        + child.dimensions.padding.top
                }
                AlignItems::Center => {
                    let outer_h = child.dimensions.margin_box().height;
                    let free_y = (self.dimensions.content.height - outer_h).max(0.0);
                    self.dimensions.content.y + free_y / 2.0
                        + child.dimensions.margin.top
                        + child.dimensions.border.top
                        + child.dimensions.padding.top
                }
                AlignItems::Stretch => {
                    if child.style.as_ref().is_none_or(|s| s.height.is_auto()) {
                        child.dimensions.content.height = (self.dimensions.content.height
                            - non_content_h)
                            .max(child.dimensions.content.height);
                    }
                    self.dimensions.content.y
                        + child.dimensions.margin.top
                        + child.dimensions.border.top
                        + child.dimensions.padding.top
                }
            };

            let delta_x = target_x - child.dimensions.content.x;
            let delta_y = target_y - child.dimensions.content.y;
            child.offset_by(delta_x, delta_y);

            current_x += child.dimensions.margin_box().width + gap + extra_spacing;
        }
    }

    /// Lays out items along a vertical main axis (Y) and horizontal cross axis (X).
    fn layout_flex_column(&mut self, containing_block: &Dimensions) {
        if self.dimensions.content.width == 0.0 {
            self.calculate_block_width(containing_block);
            self.calculate_block_position(containing_block);
        } else {
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
            }
            if self.dimensions.content.x == 0.0 && self.dimensions.content.y == 0.0 {
                self.calculate_block_position(containing_block);
            }
        }

        let gap = self.style.as_ref().map_or(0.0, |s| s.gap);
        let justify_content = self
            .style
            .as_ref()
            .map_or(JustifyContent::FlexStart, |s| s.justify_content);
        let align_items = self
            .style
            .as_ref()
            .map_or(AlignItems::Stretch, |s| s.align_items);

        let n = self.children.len();
        let total_gap = if n > 1 { (n - 1) as f32 * gap } else { 0.0 };

        // 1. Determine cross-axis size (width) and layout each child to measure height
        for child in &mut self.children {
            if let Some(ref style) = child.style {
                child.dimensions.padding.top = style.padding_top;
                child.dimensions.padding.bottom = style.padding_bottom;
                child.dimensions.padding.left = style.padding_left;
                child.dimensions.padding.right = style.padding_right;

                child.dimensions.margin.top = match style.margin_top {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.bottom = match style.margin_bottom {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.left = match style.margin_left {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
                child.dimensions.margin.right = match style.margin_right {
                    LengthOrAuto::Px(v) => v,
                    LengthOrAuto::Auto => 0.0,
                };
            }

            let non_content_w = child.dimensions.margin.left
                + child.dimensions.border.left
                + child.dimensions.padding.left
                + child.dimensions.padding.right
                + child.dimensions.border.right
                + child.dimensions.margin.right;

            let w = if let Some(w) = child.style.as_ref().and_then(|s| s.width.to_px()) {
                w
            } else if align_items == AlignItems::Stretch && self.dimensions.content.width > 0.0 {
                (self.dimensions.content.width - non_content_w).max(0.0)
            } else {
                measure_box_content_width(child)
            };
            child.dimensions.content.width = w;
            child.dimensions.content.x = self.dimensions.content.x;
            child.dimensions.content.y = self.dimensions.content.y;
            child.layout_flex_item();
        }

        let total_base_children_h: f32 = self
            .children
            .iter()
            .map(|c| c.dimensions.margin_box().height)
            .sum();
        let total_used_h = total_base_children_h + total_gap;

        let explicit_height = self.style.as_ref().and_then(|s| s.height.to_px());
        let container_h = explicit_height.unwrap_or(total_used_h);
        self.dimensions.content.height = container_h;

        // 2. Handle flex-grow along vertical main axis
        if let Some(h) = explicit_height {
            let free_space = h - total_used_h;
            let total_flex_grow: f32 = self
                .children
                .iter()
                .map(|c| c.style.as_ref().map_or(0.0, |s| s.flex_grow))
                .sum();
            if free_space > 0.0 && total_flex_grow > 0.0 {
                for child in &mut self.children {
                    let grow = child.style.as_ref().map_or(0.0, |s| s.flex_grow);
                    if grow > 0.0 {
                        let added = free_space * (grow / total_flex_grow);
                        child.dimensions.content.height += added;
                    }
                }
            }
        }

        // 3. Position items along main axis (justify-content) and cross axis (align-items)
        let final_items_h: f32 = self
            .children
            .iter()
            .map(|c| c.dimensions.margin_box().height)
            .sum::<f32>()
            + total_gap;
        let remaining_space = (container_h - final_items_h).max(0.0);

        let (mut current_y, extra_spacing) = match justify_content {
            JustifyContent::FlexStart => (self.dimensions.content.y, 0.0),
            JustifyContent::FlexEnd => (self.dimensions.content.y + remaining_space, 0.0),
            JustifyContent::Center => (self.dimensions.content.y + remaining_space / 2.0, 0.0),
            JustifyContent::SpaceBetween => {
                if n > 1 {
                    (self.dimensions.content.y, remaining_space / (n - 1) as f32)
                } else {
                    (self.dimensions.content.y, 0.0)
                }
            }
            JustifyContent::SpaceAround => {
                if n > 0 {
                    let per_item = remaining_space / n as f32;
                    (self.dimensions.content.y + per_item / 2.0, per_item)
                } else {
                    (self.dimensions.content.y, 0.0)
                }
            }
        };

        for child in &mut self.children {
            let target_y = current_y
                + child.dimensions.margin.top
                + child.dimensions.border.top
                + child.dimensions.padding.top;

            let non_content_w = child.dimensions.margin.left
                + child.dimensions.border.left
                + child.dimensions.padding.left
                + child.dimensions.padding.right
                + child.dimensions.border.right
                + child.dimensions.margin.right;

            let target_x = match align_items {
                AlignItems::FlexStart => {
                    self.dimensions.content.x
                        + child.dimensions.margin.left
                        + child.dimensions.border.left
                        + child.dimensions.padding.left
                }
                AlignItems::FlexEnd => {
                    let outer_w = child.dimensions.margin_box().width;
                    self.dimensions.content.x + (self.dimensions.content.width - outer_w)
                        + child.dimensions.margin.left
                        + child.dimensions.border.left
                        + child.dimensions.padding.left
                }
                AlignItems::Center => {
                    let outer_w = child.dimensions.margin_box().width;
                    let free_x = (self.dimensions.content.width - outer_w).max(0.0);
                    self.dimensions.content.x + free_x / 2.0
                        + child.dimensions.margin.left
                        + child.dimensions.border.left
                        + child.dimensions.padding.left
                }
                AlignItems::Stretch => {
                    if child.style.as_ref().is_none_or(|s| s.width.is_auto()) {
                        child.dimensions.content.width = (self.dimensions.content.width
                            - non_content_w)
                            .max(child.dimensions.content.width);
                    }
                    self.dimensions.content.x
                        + child.dimensions.margin.left
                        + child.dimensions.border.left
                        + child.dimensions.padding.left
                }
            };

            let delta_x = target_x - child.dimensions.content.x;
            let delta_y = target_y - child.dimensions.content.y;
            child.offset_by(delta_x, delta_y);

            current_y += child.dimensions.margin_box().height + gap + extra_spacing;
        }

        if self.dimensions.content.width == 0.0 {
            let max_w = self
                .children
                .iter()
                .map(|c| c.dimensions.margin_box().width)
                .fold(0.0f32, f32::max);
            self.dimensions.content.width = max_w;
        }
    }
}

/// Measures the intrinsic content width of a layout box.
pub fn measure_box_content_width(layout_box: &LayoutBox) -> f32 {
    if let Some(w) = layout_box.style.as_ref().and_then(|s| s.width.to_px()) {
        return w;
    }
    if let Some(ref text) = layout_box.text {
        let font_size = layout_box.style.as_ref().map_or(16.0, |s| s.font_size);
        return crate::font::measure_text(text, font_size);
    }
    if let Some(img) = layout_box.image() {
        return layout_box.dimensions.content.width.max(img.width as f32);
    }
    if !layout_box.children.is_empty() {
        let is_flex = layout_box
            .style
            .as_ref()
            .is_some_and(|s| s.display == lilies_css::Display::Flex);
        if is_flex {
            let dir = layout_box
                .style
                .as_ref()
                .map_or(lilies_css::FlexDirection::Row, |s| s.flex_direction);
            let gap = layout_box.style.as_ref().map_or(0.0, |s| s.gap);
            match dir {
                lilies_css::FlexDirection::Row => {
                    let count = layout_box.children.len();
                    let gaps = if count > 1 { (count - 1) as f32 * gap } else { 0.0 };
                    let sum: f32 = layout_box
                        .children
                        .iter()
                        .map(measure_box_outer_width)
                        .sum();
                    return sum + gaps;
                }
                lilies_css::FlexDirection::Column => {
                    return layout_box
                        .children
                        .iter()
                        .map(measure_box_outer_width)
                        .fold(0.0f32, f32::max);
                }
            }
        } else {
            let all_inline = layout_box.children.iter().all(|c| c.is_inline());
            if all_inline {
                return layout_box
                    .children
                    .iter()
                    .map(measure_box_outer_width)
                    .sum();
            } else {
                return layout_box
                    .children
                    .iter()
                    .map(measure_box_outer_width)
                    .fold(0.0f32, f32::max);
            }
        }
    }
    layout_box.dimensions.content.width
}

/// Measures the intrinsic outer width of a layout box (content + paddings + borders + margins).
pub fn measure_box_outer_width(layout_box: &LayoutBox) -> f32 {
    let content_w = measure_box_content_width(layout_box);
    let (pl, pr, ml, mr, bl, br) = if let Some(ref style) = layout_box.style {
        let ml = match style.margin_left {
            LengthOrAuto::Px(v) => v,
            LengthOrAuto::Auto => 0.0,
        };
        let mr = match style.margin_right {
            LengthOrAuto::Px(v) => v,
            LengthOrAuto::Auto => 0.0,
        };
        (
            style.padding_left,
            style.padding_right,
            ml,
            mr,
            layout_box.dimensions.border.left,
            layout_box.dimensions.border.right,
        )
    } else {
        (
            layout_box.dimensions.padding.left,
            layout_box.dimensions.padding.right,
            layout_box.dimensions.margin.left,
            layout_box.dimensions.margin.right,
            layout_box.dimensions.border.left,
            layout_box.dimensions.border.right,
        )
    };
    content_w + pl + pr + ml + mr + bl + br
}

/// Measures the intrinsic content height of a layout box.
pub fn measure_box_content_height(layout_box: &LayoutBox) -> f32 {
    if let Some(h) = layout_box.style.as_ref().and_then(|s| s.height.to_px()) {
        return h;
    }
    if layout_box.text.is_some() {
        let font_size = layout_box.style.as_ref().map_or(16.0, |s| s.font_size);
        return crate::font::line_height(font_size);
    }
    if let Some(img) = layout_box.image() {
        return layout_box.dimensions.content.height.max(img.height as f32);
    }
    if !layout_box.children.is_empty() {
        let is_flex = layout_box
            .style
            .as_ref()
            .is_some_and(|s| s.display == lilies_css::Display::Flex);
        if is_flex {
            let dir = layout_box
                .style
                .as_ref()
                .map_or(lilies_css::FlexDirection::Row, |s| s.flex_direction);
            let gap = layout_box.style.as_ref().map_or(0.0, |s| s.gap);
            match dir {
                lilies_css::FlexDirection::Row => {
                    return layout_box
                        .children
                        .iter()
                        .map(measure_box_outer_height)
                        .fold(0.0f32, f32::max);
                }
                lilies_css::FlexDirection::Column => {
                    let count = layout_box.children.len();
                    let gaps = if count > 1 { (count - 1) as f32 * gap } else { 0.0 };
                    let sum: f32 = layout_box
                        .children
                        .iter()
                        .map(measure_box_outer_height)
                        .sum();
                    return sum + gaps;
                }
            }
        } else {
            let all_inline = layout_box.children.iter().all(|c| c.is_inline());
            if all_inline {
                return layout_box
                    .children
                    .iter()
                    .map(measure_box_outer_height)
                    .fold(0.0f32, f32::max);
            } else {
                return layout_box
                    .children
                    .iter()
                    .map(measure_box_outer_height)
                    .sum();
            }
        }
    }
    layout_box.dimensions.content.height
}

/// Measures the intrinsic outer height of a layout box (content + paddings + borders + margins).
pub fn measure_box_outer_height(layout_box: &LayoutBox) -> f32 {
    let content_h = measure_box_content_height(layout_box);
    let (pt, pb, mt, mb, bt, bb) = if let Some(ref style) = layout_box.style {
        let mt = match style.margin_top {
            LengthOrAuto::Px(v) => v,
            LengthOrAuto::Auto => 0.0,
        };
        let mb = match style.margin_bottom {
            LengthOrAuto::Px(v) => v,
            LengthOrAuto::Auto => 0.0,
        };
        (
            style.padding_top,
            style.padding_bottom,
            mt,
            mb,
            layout_box.dimensions.border.top,
            layout_box.dimensions.border.bottom,
        )
    } else {
        (
            layout_box.dimensions.padding.top,
            layout_box.dimensions.padding.bottom,
            layout_box.dimensions.margin.top,
            layout_box.dimensions.margin.bottom,
            layout_box.dimensions.border.top,
            layout_box.dimensions.border.bottom,
        )
    };
    content_h + pt + pb + mt + mb + bt + bb
}
