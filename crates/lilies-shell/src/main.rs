#![deny(unsafe_code)]

use lilies_css::StyleTree;
use lilies_dom::{Document, NodeId};
use lilies_html::{load_styles_with_external, parse_html};
use lilies_js::JsRuntime;
use lilies_layout::{
    build_layout_tree_with_cache, find_parent_link, hit_test, layout, Dimensions, EdgeSizes,
    LayoutBox, Rect,
};
use lilies_net::ImageCache;
use lilies_paint::{build_display_list, rasterize, Color, DisplayItem, DisplayList, Pixmap};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowBuilder};

pub const TOOLBAR_HEIGHT: u32 = 36;

pub const DEFAULT_HTML: &str = r#"<html>
  <body style="background-color: #1e1e2e; color: #cdd6f4;">
    <div style="background-color: #313244; width: 600px; margin: 40px auto; padding: 24px;">
      <h1 style="color: #89b4fa; font-size: 32px; margin-bottom: 16px;">lilies browser</h1>
      <p style="color: #a6adc8; font-size: 16px; margin-bottom: 12px;">
        Welcome to lilies, an independent web browser rendering engine built from first principles in Rust.
      </p>
      <div style="background-color: #45475a; padding: 12px; margin-top: 16px;">
        <p style="color: #a6e3a1; font-size: 14px;">
          This text is automatically wrapped and rendered with pure software alpha blending.
        </p>
      </div>
    </div>
  </body>
</html>"#;

pub const IMAGE_DEMO_HTML: &str = r#"<html>
  <body style="background-color: #24273a; color: #cad3f5; font-size: 16px;">
    <div style="max-width: 650px; margin: 40px auto; padding: 24px; background-color: #1e2030; text-align: center;">
      <h1 style="color: #8aadf4; font-size: 28px;">lilies with Images</h1>
      <p style="margin-bottom: 20px;">Testing replaced element layout and PNG alpha transparency:</p>
      <img src="https://upload.wikimedia.org/wikipedia/commons/d/d5/Rust_programming_language_black_logo.png" alt="Rust Logo" width="120" style="margin: 10px auto;" />
      <p style="margin-top: 16px; font-size: 14px; color: #a5adcb;">The image preserves aspect ratio and scales smoothly.</p>
    </div>
  </body>
</html>"#;

pub const FLEX_DEMO_HTML: &str = r#"<html>
  <body style="background-color: #0f172a; color: #f8fafc; font-family: sans-serif; padding: 20px;">
    <!-- Flex Navbar -->
    <nav style="display: flex; justify-content: space-between; align-items: center; background-color: #1e293b; padding: 16px 24px; margin-bottom: 24px;">
      <h2 style="font-size: 20px; color: #38bdf8; margin: 0;">lilies</h2>
      <div style="display: flex; gap: 16px;">
        <span style="color: #94a3b8;">Home</span>
        <span style="color: #94a3b8;">Docs</span>
        <span style="color: #38bdf8;">GitHub</span>
      </div>
    </nav>

    <!-- Flex Cards Grid -->
    <div style="display: flex; justify-content: space-between; gap: 20px;">
      <div style="flex-grow: 1; background-color: #1e293b; padding: 20px;">
        <h3 style="color: #f1f5f9; font-size: 18px;">Card One</h3>
        <p style="color: #94a3b8; font-size: 14px;">First flex item.</p>
      </div>
      <div style="flex-grow: 1; background-color: #1e293b; padding: 20px;">
        <h3 style="color: #f1f5f9; font-size: 18px;">Card Two</h3>
        <p style="color: #94a3b8; font-size: 14px;">Second flex item.</p>
      </div>
    </div>
  </body>
</html>"#;

pub const JS_DEMO_HTML: &str = r#"<html>
  <body style="background-color: #0f172a; color: #f8fafc; font-family: sans-serif; padding: 40px; text-align: center;">
    <h1 id="heading" style="color: #38bdf8; font-size: 32px;">Static Heading</h1>
    <p id="counter-text" style="color: #94a3b8; font-size: 18px; margin: 20px auto;">Count: 0</p>
    <button id="btn" style="background-color: #3b82f6; color: white; padding: 12px 24px; font-size: 16px; border: none; cursor: pointer;">
      Click Me!
    </button>
    <script>
      console.log("Hello from lilies JavaScript runtime!");
      var heading = document.getElementById("heading");
      heading.textContent = "Dynamic Heading (Set by JS)";

      var count = 0;
      var btn = document.getElementById("btn");
      var counter = document.getElementById("counter-text");
      btn.addEventListener("click", function() {
        count = count + 1;
        counter.textContent = "Count: " + count;
        console.log("Button clicked! New count: " + count);
      });
    </script>
  </body>
</html>"#;

/// Re-runs the layout engine and rasterizer for the webpage given the viewport dimensions.
pub fn render_to_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    render_webpage_to_pixmap(doc, html_id, styles, width, height, 0.0).map(|(pixmap, _)| pixmap)
}

/// Re-runs the layout engine and rasterizer for the webpage with a vertical scroll offset.
/// Returns the rendered `Pixmap` and the `total_content_height`.
pub fn render_webpage_to_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    scroll_offset_y: f32,
) -> Option<(Pixmap, f32)> {
    render_webpage_with_layout(doc, html_id, styles, width, height, scroll_offset_y)
        .map(|(pixmap, h, _)| (pixmap, h))
}

/// Builds, lays out, and rasterizes the webpage given the viewport dimensions, scroll offset, and image cache.
/// Returns the rendered `Pixmap`, `total_content_height`, and the laid-out `LayoutBox`.
pub fn render_webpage_with_images(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    scroll_offset_y: f32,
    image_cache: &ImageCache,
) -> Option<(Pixmap, f32, LayoutBox)> {
    let mut root_box =
        build_layout_tree_with_cache(html_id, doc.arena(), styles, image_cache)?;
    let viewport = Dimensions::new(Rect::new(0.0, 0.0, width as f32, height as f32));
    layout(&mut root_box, viewport);

    let mut display_list = build_display_list(&root_box);
    let total_content_height = display_list
        .total_height()
        .max(root_box.dimensions.margin_box().height);

    if scroll_offset_y != 0.0 {
        display_list.offset(0.0, -scroll_offset_y);
    }

    let bg_color = doc
        .get_children(html_id)
        .into_iter()
        .find_map(|child| {
            if doc.node(child)?.as_element()?.tag_name() == "body" {
                styles.get(child).map(|s| s.background_color)
            } else {
                None
            }
        })
        .filter(|c| c.a > 0)
        .or_else(|| {
            styles
                .get(html_id)
                .map(|s| s.background_color)
                .filter(|c| c.a > 0)
        })
        .unwrap_or(Color::WHITE);

    let mut pixmap = Pixmap::new(width, height, bg_color);
    rasterize(&display_list, &mut pixmap);
    Some((pixmap, total_content_height, root_box))
}

/// Builds, lays out, and rasterizes the webpage given the viewport dimensions and scroll offset.
/// Returns the rendered `Pixmap`, `total_content_height`, and the laid-out `LayoutBox`.
pub fn render_webpage_with_layout(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    scroll_offset_y: f32,
) -> Option<(Pixmap, f32, LayoutBox)> {
    render_webpage_with_images(
        doc,
        html_id,
        styles,
        width,
        height,
        scroll_offset_y,
        &ImageCache::new(),
    )
}

/// Renders the complete window frame including the address bar at the top and the webpage below it.
#[allow(clippy::too_many_arguments)]
pub fn render_window_pixmap(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    current_url: &str,
    scroll_offset_y: &mut f32,
    total_content_height: &mut f32,
) -> Option<Pixmap> {
    render_window_pixmap_interactive(
        doc,
        html_id,
        styles,
        width,
        height,
        current_url,
        scroll_offset_y,
        total_content_height,
        false,
        current_url,
        current_url.len(),
    )
    .map(|(pixmap, _)| pixmap)
}

/// Renders the complete window frame with interactive address bar state.
/// Returns both the final rendered `Pixmap` and the laid-out webpage `LayoutBox`.
#[allow(clippy::too_many_arguments)]
pub fn render_window_pixmap_interactive(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    current_url: &str,
    scroll_offset_y: &mut f32,
    total_content_height: &mut f32,
    is_address_bar_focused: bool,
    address_input_buffer: &str,
    cursor_position: usize,
) -> Option<(Pixmap, LayoutBox)> {
    render_window_pixmap_interactive_with_cache(
        doc,
        html_id,
        styles,
        width,
        height,
        current_url,
        scroll_offset_y,
        total_content_height,
        is_address_bar_focused,
        address_input_buffer,
        cursor_position,
        &ImageCache::new(),
    )
}

/// Renders the complete window frame with interactive address bar state and an image cache.
#[allow(clippy::too_many_arguments)]
pub fn render_window_pixmap_interactive_with_cache(
    doc: &Document,
    html_id: NodeId,
    styles: &StyleTree,
    width: u32,
    height: u32,
    current_url: &str,
    scroll_offset_y: &mut f32,
    total_content_height: &mut f32,
    is_address_bar_focused: bool,
    address_input_buffer: &str,
    cursor_position: usize,
    image_cache: &ImageCache,
) -> Option<(Pixmap, LayoutBox)> {
    let mut window_pixmap = Pixmap::new(width, height, Color::rgb(0x18, 0x18, 0x25));

    let page_height = height.saturating_sub(TOOLBAR_HEIGHT);
    let layout_box = if page_height > 0 && width > 0 {
        let (page_pixmap, content_h, lbox) = render_webpage_with_images(
            doc,
            html_id,
            styles,
            width,
            page_height,
            *scroll_offset_y,
            image_cache,
        )?;
        *total_content_height = content_h;
        let max_scroll = (content_h - page_height as f32).max(0.0);
        *scroll_offset_y = scroll_offset_y.clamp(0.0, max_scroll);

        window_pixmap.blit(&page_pixmap, 0, TOOLBAR_HEIGHT);
        lbox
    } else {
        *total_content_height = 0.0;
        *scroll_offset_y = 0.0;
        let mut empty_box =
            build_layout_tree_with_cache(html_id, doc.arena(), styles, image_cache)?;
        let viewport = Dimensions::new(Rect::new(0.0, 0.0, width as f32, page_height as f32));
        layout(&mut empty_box, viewport);
        empty_box
    };

    let displayed_url = if is_address_bar_focused {
        address_input_buffer
    } else {
        current_url
    };

    render_address_bar(
        &mut window_pixmap,
        displayed_url,
        width,
        is_address_bar_focused,
        cursor_position,
    );

    Some((window_pixmap, layout_box))
}

/// Renders the address bar at the top of the window, including focus highlight and typing cursor.
pub fn render_address_bar(
    pixmap: &mut Pixmap,
    displayed_text: &str,
    width: u32,
    is_focused: bool,
    cursor_position: usize,
) {
    let mut bar_list = DisplayList::new();

    // 1. Toolbar background
    bar_list.push(DisplayItem::SolidColor {
        rect: Rect::new(0.0, 0.0, width as f32, TOOLBAR_HEIGHT as f32),
        color: Color::rgb(0x18, 0x18, 0x25),
    });

    // 2. Input box
    if width > 32 {
        let box_w = (width - 24) as f32;
        bar_list.push(DisplayItem::SolidColor {
            rect: Rect::new(12.0, 6.0, box_w, 24.0),
            color: Color::rgb(0x31, 0x32, 0x44),
        });

        let border_color = if is_focused {
            Color::rgb(0x89, 0xb4, 0xfa)
        } else {
            Color::rgb(0x45, 0x47, 0x5a)
        };
        bar_list.push(DisplayItem::Border {
            rect: Rect::new(12.0, 6.0, box_w, 24.0),
            widths: EdgeSizes::new(1.0, 1.0, 1.0, 1.0),
            color: border_color,
        });

        // URL text inside input box
        bar_list.push(DisplayItem::Text {
            text: displayed_text.to_string(),
            rect: Rect::new(20.0, 9.0, box_w - 16.0, 18.0),
            color: Color::rgb(0xcd, 0xd6, 0xf4),
            font_size: 13.0,
        });

        // Render visible typing cursor when focused
        if is_focused {
            let clamped_cursor = cursor_position.min(displayed_text.len());
            let safe_cursor = displayed_text
                .char_indices()
                .map(|(idx, _)| idx)
                .chain(std::iter::once(displayed_text.len()))
                .rfind(|&idx| idx <= clamped_cursor)
                .unwrap_or(0);

            let prefix = &displayed_text[..safe_cursor];
            let cursor_offset = lilies_layout::font::measure_text(prefix, 13.0);
            let cursor_x = 20.0 + cursor_offset;
            if cursor_x < 12.0 + box_w - 4.0 {
                bar_list.push(DisplayItem::SolidColor {
                    rect: Rect::new(cursor_x, 9.0, 1.5, 18.0),
                    color: Color::rgb(0x89, 0xb4, 0xfa),
                });
            }
        }
    }

    // 3. Subtle bottom border separator
    bar_list.push(DisplayItem::SolidColor {
        rect: Rect::new(0.0, (TOOLBAR_HEIGHT - 1) as f32, width as f32, 1.0),
        color: Color::rgb(0x45, 0x47, 0x5a),
    });

    rasterize(&bar_list, pixmap);
}

/// Resolves a potentially relative `href` against the `base_url`.
pub fn resolve_url(base_url: &str, href: &str) -> String {
    lilies_html::resolve_url(base_url, href)
}

/// Normalizes typed input in the address bar into a full URL.
/// Auto-prepends `https://` if no scheme is specified.
pub fn normalize_address_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed == "about:welcome" || trimmed == "about:blank" {
        return if trimmed.is_empty() {
            "about:welcome".to_string()
        } else {
            trimmed.to_string()
        };
    }

    if trimmed.contains("://") || trimmed.starts_with("file:") || trimmed.starts_with("about:") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

/// Finds all `<img src="...">` elements in the document.
pub fn find_image_elements(doc: &Document) -> Vec<(String, NodeId)> {
    let mut images = Vec::new();
    collect_image_elements(doc, doc.root(), &mut images);
    images
}

fn collect_image_elements(doc: &Document, node_id: NodeId, images: &mut Vec<(String, NodeId)>) {
    if let Some(node) = doc.node(node_id) {
        if let Some(el) = node.as_element() {
            if el.tag_name().eq_ignore_ascii_case("img") {
                if let Some(src) = el.get_attribute("src") {
                    let trimmed = src.trim();
                    if !trimmed.is_empty() {
                        images.push((trimmed.to_string(), node_id));
                    }
                }
            }
        }
    }

    for child in doc.get_children(node_id) {
        collect_image_elements(doc, child, images);
    }
}

/// Scans the document for all `<img>` tags, resolves `src` against `base_url`,
/// and fetches/decodes all images into `cache` before layout.
pub fn load_images_for_document(doc: &Document, base_url: &str, cache: &ImageCache) {
    let images = find_image_elements(doc);
    for (src, _) in images {
        let resolved = resolve_url(base_url, &src);
        match cache.fetch_and_cache(&resolved) {
            Ok(img) => {
                cache.insert(src, img);
            }
            Err(e) => {
                eprintln!("Failed to fetch or decode image '{}': {}", resolved, e);
            }
        }
    }
}

/// Source of JavaScript execution in an HTML document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    Inline(String),
    External(String),
}

/// Finds all `<script>` elements in the document in document order.
pub fn find_script_elements(doc: &Document) -> Vec<ScriptSource> {
    let mut scripts = Vec::new();
    collect_script_elements(doc, doc.root(), &mut scripts);
    scripts
}

fn collect_script_elements(doc: &Document, node_id: NodeId, scripts: &mut Vec<ScriptSource>) {
    if let Some(node) = doc.node(node_id) {
        if let Some(el) = node.as_element() {
            if el.tag_name().eq_ignore_ascii_case("script") {
                if let Some(src) = el.get_attribute("src") {
                    let trimmed = src.trim();
                    if !trimmed.is_empty() {
                        scripts.push(ScriptSource::External(trimmed.to_string()));
                        return;
                    }
                }
                let text = doc.text_content(node_id);
                if !text.trim().is_empty() {
                    scripts.push(ScriptSource::Inline(text));
                }
                return;
            }
        }
    }

    for child in doc.get_children(node_id) {
        collect_script_elements(doc, child, scripts);
    }
}

/// Executes all `<script>` blocks and external scripts for a document in document order.
pub fn run_scripts_for_document(runtime: &mut JsRuntime, base_url: &str) {
    runtime.set_location_url(base_url);
    let scripts = find_script_elements(&runtime.doc());
    for script in scripts {
        match script {
            ScriptSource::Inline(code) => {
                if let Err(e) = runtime.eval(&code) {
                    eprintln!("JavaScript execution error: {}", e);
                }
            }
            ScriptSource::External(src) => {
                let resolved = resolve_url(base_url, &src);
                match lilies_net::fetch(&resolved) {
                    Ok(code) => {
                        if let Err(e) = runtime.eval(&code) {
                            eprintln!("JavaScript execution error in '{}': {}", resolved, e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch external script '{}': {}", resolved, e);
                    }
                }
            }
        }
    }
}

/// Cached rendering of the webpage for fast compositing during address bar interaction.
#[derive(Clone, Debug)]
pub struct CachedPage {
    pub pixmap: Pixmap,
    pub layout_box: LayoutBox,
    pub width: u32,
    pub page_height: u32,
    pub scroll_offset_y: f32,
    pub total_content_height: f32,
}

/// Shell state holding browser DOM, styles, navigation history, and interactive UI state.
pub struct ShellState {
    pub current_url: String,
    pub doc: Document,
    pub styles: StyleTree,
    pub image_cache: ImageCache,
    pub html_id: NodeId,
    pub current_layout_box: Option<LayoutBox>,
    pub scroll_offset_y: f32,
    pub total_content_height: f32,

    pub is_address_bar_focused: bool,
    pub address_input_buffer: String,
    pub cursor_position: usize,

    pub history: Vec<String>,
    pub history_index: usize,

    pub mouse_pos: (f64, f64),
    pub alt_pressed: bool,

    pub cached_page: Option<CachedPage>,
    pub cached_window: Option<Pixmap>,

    pub js_runtime: JsRuntime,
}

impl ShellState {
    pub fn new(initial_target: String) -> Self {
        let (html_content, current_url) = fetch_or_fallback(&initial_target);
        let doc = parse_html(&html_content);
        let mut styles = load_styles_with_external(&doc, &current_url);
        let image_cache = ImageCache::new();
        load_images_for_document(&doc, &current_url, &image_cache);

        let mut js_runtime = JsRuntime::new(doc);
        run_scripts_for_document(&mut js_runtime, &current_url);
        if js_runtime.take_dirty() {
            styles = load_styles_with_external(&js_runtime.doc(), &current_url);
        }
        let doc = js_runtime.doc().clone();
        let html_id = find_html_root(&doc).expect("Root <html> element required in document");

        Self {
            address_input_buffer: current_url.clone(),
            cursor_position: current_url.len(),
            history: vec![current_url.clone()],
            history_index: 0,
            current_url,
            doc,
            styles,
            image_cache,
            html_id,
            current_layout_box: None,
            scroll_offset_y: 0.0,
            total_content_height: 0.0,
            is_address_bar_focused: false,
            mouse_pos: (0.0, 0.0),
            alt_pressed: false,
            cached_page: None,
            cached_window: None,
            js_runtime,
        }
    }

    pub fn navigate(&mut self, target_url: &str, push_history: bool) {
        println!("Navigating to: {}", target_url);
        let (html_content, loaded_url) = fetch_or_fallback(target_url);
        let doc = parse_html(&html_content);
        let mut styles = load_styles_with_external(&doc, &loaded_url);
        load_images_for_document(&doc, &loaded_url, &self.image_cache);

        let mut js_runtime = JsRuntime::new(doc);
        run_scripts_for_document(&mut js_runtime, &loaded_url);
        if js_runtime.take_dirty() {
            styles = load_styles_with_external(&js_runtime.doc(), &loaded_url);
        }
        let doc = js_runtime.doc().clone();
        let Some(html_id) = find_html_root(&doc) else {
            eprintln!("Document missing <html> root element");
            return;
        };

        self.doc = doc;
        self.styles = styles;
        self.html_id = html_id;
        self.current_url = loaded_url;
        self.js_runtime = js_runtime;
        self.address_input_buffer = self.current_url.clone();
        self.cursor_position = self.address_input_buffer.len();
        self.scroll_offset_y = 0.0;
        self.is_address_bar_focused = false;
        self.cached_page = None;
        self.cached_window = None;

        if push_history {
            self.history.truncate(self.history_index + 1);
            self.history.push(self.current_url.clone());
            self.history_index = self.history.len() - 1;
        }
    }

    pub fn go_back(&mut self) -> bool {
        if self.history_index > 0 {
            self.history_index -= 1;
            let prev_url = self.history[self.history_index].clone();
            self.navigate(&prev_url, false);
            true
        } else {
            false
        }
    }

    pub fn handle_char_input(&mut self, text: &str) {
        let chars_to_insert: String = text.chars().filter(|c| !c.is_control()).collect();
        if !chars_to_insert.is_empty() {
            let clamped = self.cursor_position.min(self.address_input_buffer.len());
            let safe_idx = self
                .address_input_buffer
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(self.address_input_buffer.len()))
                .rfind(|&i| i <= clamped)
                .unwrap_or(0);

            self.address_input_buffer
                .insert_str(safe_idx, &chars_to_insert);
            self.cursor_position = safe_idx + chars_to_insert.len();
        }
    }

    pub fn handle_backspace(&mut self) -> bool {
        if self.cursor_position > 0 {
            let clamped = self.cursor_position.min(self.address_input_buffer.len());
            let prev_idx = self.address_input_buffer[..clamped]
                .char_indices()
                .last()
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            self.address_input_buffer.remove(prev_idx);
            self.cursor_position = prev_idx;
            true
        } else {
            false
        }
    }

    /// Renders the complete browser window frame with caching for the webpage.
    ///
    /// When only address bar state changes (e.g. while typing, moving the cursor, or focusing),
    /// the rendered webpage pixels and layout tree are reused from the cache, avoiding expensive
    /// layout recomputation and rasterization on every keystroke.
    pub fn render_window_frame(&mut self, width: u32, height: u32) -> Option<&Pixmap> {
        let page_height = height.saturating_sub(TOOLBAR_HEIGHT);

        // 1. Check if cached page is valid
        let need_page_render = match &self.cached_page {
            Some(cached) => {
                cached.width != width
                    || cached.page_height != page_height
                    || (cached.scroll_offset_y - self.scroll_offset_y).abs() > 0.001
            }
            None => true,
        };

        if need_page_render {
            if page_height > 0 && width > 0 {
                let (page_pixmap, content_h, lbox) = render_webpage_with_images(
                    &self.doc,
                    self.html_id,
                    &self.styles,
                    width,
                    page_height,
                    self.scroll_offset_y,
                    &self.image_cache,
                )?;
                self.total_content_height = content_h;
                let max_scroll = (content_h - page_height as f32).max(0.0);
                self.scroll_offset_y = self.scroll_offset_y.clamp(0.0, max_scroll);
                self.current_layout_box = Some(lbox.clone());

                self.cached_page = Some(CachedPage {
                    pixmap: page_pixmap,
                    layout_box: lbox,
                    width,
                    page_height,
                    scroll_offset_y: self.scroll_offset_y,
                    total_content_height: content_h,
                });
            } else {
                self.total_content_height = 0.0;
                self.scroll_offset_y = 0.0;
                let mut empty_box = build_layout_tree_with_cache(
                    self.html_id,
                    self.doc.arena(),
                    &self.styles,
                    &self.image_cache,
                )?;
                let viewport =
                    Dimensions::new(Rect::new(0.0, 0.0, width as f32, page_height as f32));
                layout(&mut empty_box, viewport);
                self.current_layout_box = Some(empty_box.clone());

                let empty_pixmap = Pixmap::new(width, page_height, Color::rgb(0x18, 0x18, 0x25));
                self.cached_page = Some(CachedPage {
                    pixmap: empty_pixmap,
                    layout_box: empty_box,
                    width,
                    page_height,
                    scroll_offset_y: 0.0,
                    total_content_height: 0.0,
                });
            }

            // Page re-rendered: reconstruct window pixmap with new webpage blit
            let mut window_pixmap = Pixmap::new(width, height, Color::rgb(0x18, 0x18, 0x25));
            if let Some(cached) = &self.cached_page {
                window_pixmap.blit(&cached.pixmap, 0, TOOLBAR_HEIGHT);
            }
            self.cached_window = Some(window_pixmap);
        } else if self.current_layout_box.is_none() {
            if let Some(cached) = &self.cached_page {
                self.current_layout_box = Some(cached.layout_box.clone());
            }
        }

        // 2. Ensure cached_window exists and has matching dimensions
        let window_needs_rebuild = match &self.cached_window {
            Some(win) => win.width != width || win.height != height,
            None => true,
        };
        if window_needs_rebuild {
            let mut window_pixmap = Pixmap::new(width, height, Color::rgb(0x18, 0x18, 0x25));
            if let Some(cached) = &self.cached_page {
                window_pixmap.blit(&cached.pixmap, 0, TOOLBAR_HEIGHT);
            }
            self.cached_window = Some(window_pixmap);
        }

        // 3. Render address bar onto cached_window (extremely fast <0.1ms)
        let displayed_url = if self.is_address_bar_focused {
            &self.address_input_buffer
        } else {
            &self.current_url
        };

        if let Some(window_pixmap) = &mut self.cached_window {
            render_address_bar(
                window_pixmap,
                displayed_url,
                width,
                self.is_address_bar_focused,
                self.cursor_position,
            );
        }

        self.cached_window.as_ref()
    }
}

fn fetch_or_fallback(target: &str) -> (String, String) {
    if target == "about:blank" {
        return (
            "<html><body style=\"background-color: #1e1e2e;\"></body></html>".to_string(),
            "about:blank".to_string(),
        );
    }
    if target == "about:welcome" || target.is_empty() {
        return (DEFAULT_HTML.to_string(), "about:welcome".to_string());
    }
    if target == "about:flex" {
        return (FLEX_DEMO_HTML.to_string(), "about:flex".to_string());
    }
    if target == "about:images" {
        return (IMAGE_DEMO_HTML.to_string(), "about:images".to_string());
    }
    if target == "about:js" {
        return (JS_DEMO_HTML.to_string(), "about:js".to_string());
    }

    match lilies_net::fetch(target) {
        Ok(html) => {
            println!(
                "Successfully fetched {} bytes from '{}'",
                html.len(),
                target
            );
            (html, target.to_string())
        }
        Err(err) => {
            eprintln!(
                "Could not fetch '{}': {}. Falling back to default demo HTML.",
                target, err
            );
            (DEFAULT_HTML.to_string(), target.to_string())
        }
    }
}

fn find_html_root(doc: &Document) -> Option<NodeId> {
    doc.get_children(doc.root())
        .into_iter()
        .find(|&id| {
            doc.node(id)
                .and_then(|n| n.as_element())
                .is_some_and(|el| el.tag_name() == "html")
        })
        .or_else(|| {
            doc.get_children(doc.root())
                .into_iter()
                .find(|&id| doc.node(id).and_then(|n| n.as_element()).is_some())
        })
}

/// Copies RGBA pixels from `Pixmap` into a `softbuffer` `u32` slice in `0x00RRGGBB` format.
pub fn copy_pixmap_to_softbuffer(pixmap: &Pixmap, buffer: &mut [u32]) {
    for (chunk, dst) in pixmap
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(buffer.iter_mut())
    {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        *dst = (r << 16) | (g << 8) | b;
    }
}

/// Executes dynamic layout, rasterization, and presentation onto the window surface.
fn render_frame(
    surface: &mut Surface<Arc<Window>, Arc<Window>>,
    state: &mut ShellState,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
        return Ok(());
    };

    surface.resize(w, h)?;

    let pixmap = state
        .render_window_frame(width, height)
        .ok_or("Failed to build layout tree from root")?;

    let mut buffer = surface.buffer_mut()?;
    copy_pixmap_to_softbuffer(pixmap, &mut buffer);
    buffer.present()?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lilies: Modern Browser Engine (Milestone 10 - Fast Interactive Navigation) ===");

    // Step 1: Parse CLI argument or default to welcome page
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "about:welcome".to_string());
    println!("Loading: {}", target);

    let mut state = ShellState::new(target);

    println!(
        "Document parsed successfully (DOM nodes: {})",
        state.doc.arena().len()
    );

    // Step 2: Initialize EventLoop and Window titled "lilies"
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("lilies")
            .with_inner_size(LogicalSize::new(800.0, 600.0))
            .build(&event_loop)?,
    );

    // Step 3: Initialize softbuffer Context and Surface
    let context = Context::new(window.clone())?;
    let mut surface = Surface::new(&context, window.clone())?;

    // Initial render with current window size
    let size = window.inner_size();
    if let Err(e) = render_frame(&mut surface, &mut state, size.width, size.height) {
        eprintln!("Initial render warning: {e}");
    }

    // Step 4: Run event loop
    event_loop.run(move |event, elwt| match event {
        Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
            WindowEvent::Resized(physical_size) => {
                if let Err(e) = render_frame(
                    &mut surface,
                    &mut state,
                    physical_size.width,
                    physical_size.height,
                ) {
                    eprintln!("Render error on resize: {e}");
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => -y * 36.0,
                    MouseScrollDelta::PixelDelta(pos) => -pos.y as f32,
                };
                let old_scroll = state.scroll_offset_y;
                let page_height = window.inner_size().height.saturating_sub(TOOLBAR_HEIGHT) as f32;
                let max_scroll = (state.total_content_height - page_height).max(0.0);
                state.scroll_offset_y =
                    (state.scroll_offset_y + scroll_amount).clamp(0.0, max_scroll);
                if (state.scroll_offset_y - old_scroll).abs() > 0.001 {
                    let (mx, my) = (state.mouse_pos.0 as f32, state.mouse_pos.1 as f32);
                    if my >= TOOLBAR_HEIGHT as f32 {
                        if let Some(ref lbox) = state.current_layout_box {
                            let page_x = mx;
                            let page_y = (my - TOOLBAR_HEIGHT as f32) + state.scroll_offset_y;
                            let hovered = hit_test(lbox, page_x, page_y);
                            let link =
                                hovered.and_then(|id| find_parent_link(id, state.doc.arena()));
                            if link.is_some() {
                                window.set_cursor_icon(winit::window::CursorIcon::Pointer);
                            } else {
                                window.set_cursor_icon(winit::window::CursorIcon::Default);
                            }
                        }
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.mouse_pos = (position.x, position.y);
                let mx = position.x as f32;
                let my = position.y as f32;

                if my < TOOLBAR_HEIGHT as f32 {
                    window.set_cursor_icon(winit::window::CursorIcon::Default);
                } else if let Some(ref lbox) = state.current_layout_box {
                    let page_x = mx;
                    let page_y = (my - TOOLBAR_HEIGHT as f32) + state.scroll_offset_y;
                    let hovered = hit_test(lbox, page_x, page_y);
                    let link = hovered.and_then(|id| find_parent_link(id, state.doc.arena()));
                    if link.is_some() {
                        window.set_cursor_icon(winit::window::CursorIcon::Pointer);
                    } else {
                        window.set_cursor_icon(winit::window::CursorIcon::Default);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (mx, my) = (state.mouse_pos.0 as f32, state.mouse_pos.1 as f32);
                if my < TOOLBAR_HEIGHT as f32 {
                    if !state.is_address_bar_focused {
                        state.is_address_bar_focused = true;
                        state.address_input_buffer = state.current_url.clone();
                        state.cursor_position = state.address_input_buffer.len();
                        window.request_redraw();
                    }
                } else {
                    let was_focused = state.is_address_bar_focused;
                    state.is_address_bar_focused = false;

                    if let Some(ref lbox) = state.current_layout_box {
                        let page_x = mx;
                        let page_y = (my - TOOLBAR_HEIGHT as f32) + state.scroll_offset_y;
                        let hit = hit_test(lbox, page_x, page_y);
                        let link = hit.and_then(|id| find_parent_link(id, state.doc.arena()));
                        if let Some(href) = link {
                            let resolved = resolve_url(&state.current_url, &href);
                            state.navigate(&resolved, true);
                            window.request_redraw();
                        } else if let Some(hit_id) = hit {
                            if let Err(e) = state.js_runtime.dispatch_click(hit_id) {
                                eprintln!("JS error on click: {}", e);
                            }
                            if state.js_runtime.take_dirty() {
                                state.doc = state.js_runtime.doc().clone();
                                state.styles =
                                    load_styles_with_external(&state.doc, &state.current_url);
                                state.cached_page = None;
                                state.cached_window = None;
                                window.request_redraw();
                            } else if was_focused {
                                window.request_redraw();
                            }
                        } else if was_focused {
                            window.request_redraw();
                        }
                    } else if was_focused {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if key_event.state == ElementState::Pressed {
                    match &key_event.logical_key {
                        Key::Named(NamedKey::Enter) => {
                            if state.is_address_bar_focused {
                                let target = normalize_address_input(&state.address_input_buffer);
                                state.navigate(&target, true);
                                state.is_address_bar_focused = false;
                                window.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if state.is_address_bar_focused {
                                if state.handle_backspace() {
                                    window.request_redraw();
                                }
                            } else if state.go_back() {
                                window.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if state.alt_pressed {
                                if state.go_back() {
                                    window.request_redraw();
                                }
                            } else if state.is_address_bar_focused && state.cursor_position > 0 {
                                let clamped =
                                    state.cursor_position.min(state.address_input_buffer.len());
                                let prev_idx = state.address_input_buffer[..clamped]
                                    .char_indices()
                                    .last()
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(0);
                                state.cursor_position = prev_idx;
                                window.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if state.is_address_bar_focused
                                && state.cursor_position < state.address_input_buffer.len()
                            {
                                if let Some(ch) = state.address_input_buffer
                                    [state.cursor_position..]
                                    .chars()
                                    .next()
                                {
                                    state.cursor_position += ch.len_utf8();
                                    window.request_redraw();
                                }
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if state.is_address_bar_focused {
                                state.cursor_position = 0;
                                window.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if state.is_address_bar_focused {
                                state.cursor_position = state.address_input_buffer.len();
                                window.request_redraw();
                            }
                        }
                        Key::Character(s) => {
                            if state.is_address_bar_focused {
                                state.handle_char_input(s);
                                window.request_redraw();
                            }
                        }
                        _ => {
                            if state.is_address_bar_focused {
                                if let Some(ref text) = key_event.text {
                                    state.handle_char_input(text);
                                    window.request_redraw();
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(ref s)) => {
                if state.is_address_bar_focused {
                    state.handle_char_input(s);
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.alt_pressed = modifiers.state().alt_key();
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                if let Err(e) = render_frame(&mut surface, &mut state, size.width, size.height) {
                    eprintln!("Render error on redraw: {e}");
                }
            }
            WindowEvent::CloseRequested => {
                elwt.exit();
            }
            _ => {}
        },
        _ => {}
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use lilies_css::{
        compute_styles, compute_styles_for_document, parse_styles_from_document, StyleSheet,
    };
    use lilies_layout::build_layout_tree;

    #[test]
    fn test_copy_pixmap_to_softbuffer() {
        let mut pixmap = Pixmap::new(2, 2, Color::WHITE);
        pixmap.set_pixel(0, 0, Color::rgba(0x2b, 0x3a, 0x4a, 0xff));
        pixmap.set_pixel(1, 0, Color::rgba(0xff, 0x57, 0x22, 0xff));
        pixmap.set_pixel(0, 1, Color::rgba(0x00, 0x00, 0x00, 0xff));
        pixmap.set_pixel(1, 1, Color::rgba(0xff, 0xff, 0xff, 0xff));

        let mut buffer = vec![0u32; 4];
        copy_pixmap_to_softbuffer(&pixmap, &mut buffer);

        assert_eq!(buffer[0], 0x002B3A4A);
        assert_eq!(buffer[1], 0x00FF5722);
        assert_eq!(buffer[2], 0x00000000);
        assert_eq!(buffer[3], 0x00FFFFFF);
    }

    #[test]
    fn test_card_centering_across_different_viewport_sizes() {
        const CARD_HTML: &str = r#"<html>
  <body style="background-color: #ffffff;">
    <div class="card" style="background-color: #2b3a4a; width: 400px; height: 300px; margin: 50px auto;">
      <div class="inner" style="background-color: #ff5722; width: 200px; height: 100px; margin: 20px;"></div>
    </div>
  </body>
</html>"#;
        let doc = parse_html(CARD_HTML);
        let stylesheet = StyleSheet::new();
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

        let pixmap_800 = render_to_pixmap(&doc, html_id, &styles, 800, 600).expect("pixmap");
        assert_eq!(pixmap_800.width, 800);
        assert_eq!(pixmap_800.height, 600);

        let card_color = Color::rgba(0x2b, 0x3a, 0x4a, 0xff);
        let bg_color = Color::rgba(0xff, 0xff, 0xff, 0xff);

        // At y = 60 (inside card vertically, margin-top is 50):
        assert_eq!(pixmap_800.get_pixel(199, 60), Some(bg_color));
        assert_eq!(pixmap_800.get_pixel(200, 60), Some(card_color));
        assert_eq!(pixmap_800.get_pixel(599, 60), Some(card_color));
        assert_eq!(pixmap_800.get_pixel(600, 60), Some(bg_color));

        let pixmap_1000 = render_to_pixmap(&doc, html_id, &styles, 1000, 600).expect("pixmap");
        assert_eq!(pixmap_1000.width, 1000);
        assert_eq!(pixmap_1000.height, 600);

        assert_eq!(pixmap_1000.get_pixel(299, 60), Some(bg_color));
        assert_eq!(pixmap_1000.get_pixel(300, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(699, 60), Some(card_color));
        assert_eq!(pixmap_1000.get_pixel(700, 60), Some(bg_color));

        let pixmap_600 = render_to_pixmap(&doc, html_id, &styles, 600, 600).expect("pixmap");
        assert_eq!(pixmap_600.width, 600);
        assert_eq!(pixmap_600.height, 600);

        assert_eq!(pixmap_600.get_pixel(99, 60), Some(bg_color));
        assert_eq!(pixmap_600.get_pixel(100, 60), Some(card_color));
        assert_eq!(pixmap_600.get_pixel(499, 60), Some(card_color));
        assert_eq!(pixmap_600.get_pixel(500, 60), Some(bg_color));
    }

    #[test]
    fn test_milestone6_rich_document_render() {
        let doc = parse_html(DEFAULT_HTML);
        let stylesheet = parse_styles_from_document(&doc);
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

        let pixmap = render_to_pixmap(&doc, html_id, &styles, 800, 600).expect("pixmap");
        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);

        let body_bg = Color::rgba(0x1e, 0x1e, 0x2e, 0xff);
        let container_bg = Color::rgba(0x31, 0x32, 0x44, 0xff);

        assert_eq!(pixmap.get_pixel(50, 50), Some(body_bg));
        assert_eq!(pixmap.get_pixel(80, 45), Some(container_bg));

        assert_eq!(pixmap.get_pixel(75, 50), Some(body_bg));
        assert_eq!(pixmap.get_pixel(76, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(723, 50), Some(container_bg));
        assert_eq!(pixmap.get_pixel(724, 50), Some(body_bg));

        let text_pixels_count = pixmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|chunk| {
                let color = Color::rgba(chunk[0], chunk[1], chunk[2], chunk[3]);
                color != body_bg && color != container_bg
            })
            .count();

        assert!(
            text_pixels_count > 100,
            "Expected rasterized text pixels, found {}",
            text_pixels_count
        );
    }

    #[test]
    fn test_milestone6_dynamic_text_reflow_on_resize() {
        let doc = parse_html(DEFAULT_HTML);
        let stylesheet = StyleSheet::new();
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

        let mut root_800 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_800,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );

        let mut root_1200 = build_layout_tree(html_id, doc.arena(), &styles).unwrap();
        layout(
            &mut root_1200,
            Dimensions::new(Rect::new(0.0, 0.0, 1200.0, 800.0)),
        );

        let body_800 = &root_800.children[0];
        let card_800 = &body_800.children[0];
        assert_eq!(card_800.dimensions.margin.left, 76.0);
        assert_eq!(card_800.dimensions.content.width, 600.0);
        assert_eq!(card_800.dimensions.content.x, 100.0);

        let body_1200 = &root_1200.children[0];
        let card_1200 = &body_1200.children[0];
        assert_eq!(card_1200.dimensions.margin.left, 276.0);
        assert_eq!(card_1200.dimensions.content.width, 600.0);
        assert_eq!(card_1200.dimensions.content.x, 300.0);

        let auto_html = r#"<html><body><p>One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen</p></body></html>"#;
        let auto_doc = parse_html(auto_html);
        let auto_styles = compute_styles(&auto_doc, &stylesheet);
        let auto_html_id = auto_doc
            .get_children(auto_doc.root())
            .into_iter()
            .find(|&id| {
                auto_doc
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut wide_box = build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut wide_box,
            Dimensions::new(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );
        let wide_p = &wide_box.children[0].children[0];
        let wide_text = &wide_p.children[0];
        let wide_line_count = wide_text.lines.len();

        let mut narrow_box =
            build_layout_tree(auto_html_id, auto_doc.arena(), &auto_styles).unwrap();
        layout(
            &mut narrow_box,
            Dimensions::new(Rect::new(0.0, 0.0, 150.0, 600.0)),
        );
        let narrow_p = &narrow_box.children[0].children[0];
        let narrow_text = &narrow_p.children[0];
        let narrow_line_count = narrow_text.lines.len();

        assert!(narrow_line_count > wide_line_count);
        assert!(narrow_p.dimensions.content.height > wide_p.dimensions.content.height);
    }

    #[test]
    fn test_vertical_scrolling_offsets_page_content() {
        let html = r#"<html>
          <body style="background-color: #000000; margin: 0px; padding: 0px;">
            <div id="first" style="background-color: #ff0000; width: 400px; height: 200px;"></div>
            <div id="second" style="background-color: #0000ff; width: 400px; height: 200px;"></div>
          </body>
        </html>"#;

        let doc = parse_html(html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        // 1. Render at scroll_offset_y = 0:
        let (pixmap_top, total_h) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 0.0).expect("pixmap_top");
        assert_eq!(total_h, 400.0);

        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);

        // At y = 50, first box (red) is visible
        assert_eq!(pixmap_top.get_pixel(50, 50), Some(red));
        // At y = 250, second box (blue) is visible
        assert_eq!(pixmap_top.get_pixel(50, 250), Some(blue));

        // 2. Render scrolled down by 100px:
        let (pixmap_scrolled, _) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 100.0)
                .expect("pixmap_scrolled");

        // The first box was y in [0, 200), now offset by -100px -> y in [-100, 100).
        // The second box was y in [200, 400), now offset by -100px -> y in [100, 300).
        // At y = 50, red box is still there:
        assert_eq!(pixmap_scrolled.get_pixel(50, 50), Some(red));
        // At y = 150, blue box has scrolled up into view!
        assert_eq!(pixmap_scrolled.get_pixel(50, 150), Some(blue));

        // 3. Render scrolled down by 200px:
        let (pixmap_scrolled_200, _) =
            render_webpage_to_pixmap(&doc, html_id, &styles, 400, 300, 200.0)
                .expect("pixmap_scrolled_200");

        // At y = 50, blue box is now at the top!
        assert_eq!(pixmap_scrolled_200.get_pixel(50, 50), Some(blue));
    }

    #[test]
    fn test_window_address_bar_rendering_and_dimensions() {
        let html = r#"<html><body style="background-color: #ffffff;"><p>Hello</p></body></html>"#;
        let doc = parse_html(html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut scroll_offset_y = 0.0;
        let mut total_content_h = 0.0;
        let url = "https://example.com";

        let window_pixmap = render_window_pixmap(
            &doc,
            html_id,
            &styles,
            800,
            600,
            url,
            &mut scroll_offset_y,
            &mut total_content_h,
        )
        .expect("window pixmap");

        assert_eq!(window_pixmap.width, 800);
        assert_eq!(window_pixmap.height, 600);

        // Top 36px is toolbar:
        // Background color of toolbar at x = 2, y = 2:
        assert_eq!(
            window_pixmap.get_pixel(2, 2),
            Some(Color::rgb(0x18, 0x18, 0x25))
        );

        // Separator line at y = TOOLBAR_HEIGHT - 1 (y = 35):
        assert_eq!(
            window_pixmap.get_pixel(100, TOOLBAR_HEIGHT - 1),
            Some(Color::rgb(0x45, 0x47, 0x5a))
        );

        // Below toolbar (y = 40): webpage background (white)
        assert_eq!(
            window_pixmap.get_pixel(100, TOOLBAR_HEIGHT + 4),
            Some(Color::WHITE)
        );
    }

    #[test]
    #[ignore]
    fn test_e2e_fetch_and_render_example_com() {
        let html = lilies_net::fetch("https://example.com").expect("fetch example.com");
        let doc = parse_html(&html);
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut scroll_offset_y = 0.0;
        let mut total_content_height = 0.0;
        let pixmap = render_window_pixmap(
            &doc,
            html_id,
            &styles,
            800,
            600,
            "https://example.com",
            &mut scroll_offset_y,
            &mut total_content_height,
        )
        .expect("render window pixmap");

        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);
        assert!(total_content_height > 0.0);

        let _ = pixmap.save_ppm("../../output_example_com.ppm");
    }

    #[test]
    fn test_resolve_url_variations() {
        // Absolute URL replaces base
        assert_eq!(
            resolve_url(
                "https://example.com/dir/page.html",
                "https://other.com/test"
            ),
            "https://other.com/test"
        );

        // Root-relative URL resolves against origin
        assert_eq!(
            resolve_url("https://example.com/dir/page.html", "/about"),
            "https://example.com/about"
        );

        // Relative path resolves against directory
        assert_eq!(
            resolve_url("https://example.com/dir/page.html", "sub.html"),
            "https://example.com/dir/sub.html"
        );

        // Trailing slash base
        assert_eq!(
            resolve_url("https://example.com/dir/", "sub.html"),
            "https://example.com/dir/sub.html"
        );

        // Fragment / hash
        assert_eq!(
            resolve_url("https://example.com/page.html", "#section1"),
            "https://example.com/page.html#section1"
        );

        // Empty href retains base
        assert_eq!(
            resolve_url("https://example.com", ""),
            "https://example.com"
        );

        // Base with offline demo suffix
        assert_eq!(
            resolve_url("https://example.com/page.html (offline demo)", "/faq"),
            "https://example.com/faq"
        );
    }

    #[test]
    fn test_normalize_address_input_variations() {
        assert_eq!(
            normalize_address_input("example.com"),
            "https://example.com"
        );
        assert_eq!(
            normalize_address_input("example.com/docs/api"),
            "https://example.com/docs/api"
        );
        assert_eq!(
            normalize_address_input("https://iana.org"),
            "https://iana.org"
        );
        assert_eq!(
            normalize_address_input("http://localhost:8080"),
            "http://localhost:8080"
        );
        assert_eq!(
            normalize_address_input("file:///var/log/syslog"),
            "file:///var/log/syslog"
        );
        assert_eq!(
            normalize_address_input("   crates.io/crates/lilies   "),
            "https://crates.io/crates/lilies"
        );
        assert_eq!(normalize_address_input("   "), "about:welcome");
        assert_eq!(normalize_address_input("about:welcome"), "about:welcome");
        assert_eq!(normalize_address_input("about:blank"), "about:blank");
    }

    #[test]
    fn test_navigation_history_flow() {
        // Construct state offline without hitting network
        let mut state = ShellState::new("file:///offline_initial".to_string());
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history_index, 0);
        assert!(!state.go_back()); // Cannot go back at beginning

        // Navigate to second page
        state.navigate("file:///page2", true);
        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history_index, 1);
        assert_eq!(state.current_url, "file:///page2");

        // Navigate to third page
        state.navigate("file:///page3", true);
        assert_eq!(state.history.len(), 3);
        assert_eq!(state.history_index, 2);
        assert_eq!(state.current_url, "file:///page3");

        // Go back to second page
        assert!(state.go_back());
        assert_eq!(state.history_index, 1);
        assert_eq!(state.current_url, "file:///page2");

        // Go back to first page
        assert!(state.go_back());
        assert_eq!(state.history_index, 0);
        assert_eq!(state.current_url, "file:///offline_initial");

        // Cannot go back further
        assert!(!state.go_back());
        assert_eq!(state.history_index, 0);

        // Branching: navigate to new page from index 0
        state.navigate("file:///branched", true);
        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history_index, 1);
        assert_eq!(state.history[0], "file:///offline_initial");
        assert_eq!(state.history[1], "file:///branched");
    }

    #[test]
    fn test_shell_state_text_editing() {
        let mut state = ShellState::new("file:///offline".to_string());
        state.is_address_bar_focused = true;
        state.address_input_buffer = "hello".to_string();
        state.cursor_position = 5;

        // Type " world"
        state.handle_char_input(" world");
        assert_eq!(state.address_input_buffer, "hello world");
        assert_eq!(state.cursor_position, 11);

        // Backspace removes 'd'
        assert!(state.handle_backspace());
        assert_eq!(state.address_input_buffer, "hello worl");
        assert_eq!(state.cursor_position, 10);

        // Move cursor to position 5 and insert '!'
        state.cursor_position = 5;
        state.handle_char_input("!");
        assert_eq!(state.address_input_buffer, "hello! worl");
        assert_eq!(state.cursor_position, 6);
    }

    #[test]
    fn test_address_bar_focus_rendering_and_cursor() {
        let doc = parse_html("<html><body><p>Test</p></body></html>");
        let styles = compute_styles_for_document(&doc);
        let html_id = doc
            .get_children(doc.root())
            .into_iter()
            .find(|&id| {
                doc.node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "html")
            })
            .unwrap();

        let mut scroll_offset_y = 0.0;
        let mut total_content_height = 0.0;

        // 1. Unfocused render: border is #45475a
        let (pixmap_unfocused, _) = render_window_pixmap_interactive(
            &doc,
            html_id,
            &styles,
            800,
            600,
            "https://example.com",
            &mut scroll_offset_y,
            &mut total_content_height,
            false,
            "https://example.com",
            19,
        )
        .expect("unfocused pixmap");

        // Top border of input box is at y = 6, x between 12 and 788
        assert_eq!(
            pixmap_unfocused.get_pixel(15, 6),
            Some(Color::rgb(0x45, 0x47, 0x5a))
        );

        // 2. Focused render: border is #89b4fa
        let (pixmap_focused, _) = render_window_pixmap_interactive(
            &doc,
            html_id,
            &styles,
            800,
            600,
            "https://example.com",
            &mut scroll_offset_y,
            &mut total_content_height,
            true,
            "https://example.com",
            0, // cursor at start
        )
        .expect("focused pixmap");

        // Top border of input box should now be highlight color
        assert_eq!(
            pixmap_focused.get_pixel(15, 6),
            Some(Color::rgb(0x89, 0xb4, 0xfa))
        );

        // Cursor at position 0 is at x = 20, y in 9..27
        assert_eq!(
            pixmap_focused.get_pixel(20, 10),
            Some(Color::rgb(0x89, 0xb4, 0xfa))
        );
    }

    #[test]
    fn test_shell_external_stylesheet_loading_e2e() {
        use lilies_css::TextAlign;
        use std::fs;

        let temp_dir = std::env::temp_dir().join("lilies_milestone9_test");
        let _ = fs::create_dir_all(&temp_dir);

        let css_file = temp_dir.join("site.css");
        let html_file = temp_dir.join("index.html");

        fs::write(
            &css_file,
            "body { background-color: #34495e; max-width: 30em; text-align: center; }\nh1 { color: #e74c3c; font-size: 2em; }",
        )
        .expect("write css");

        fs::write(
            &html_file,
            r#"<html><head><link rel="stylesheet" href="site.css"></head><body><h1>Hello World</h1></body></html>"#,
        )
        .expect("write html");

        let target_url = format!("file://{}", html_file.display());
        let state = ShellState::new(target_url);

        // Find body and h1 in the document
        let body_id = state
            .doc
            .get_children(state.html_id)
            .into_iter()
            .find(|&id| {
                state
                    .doc
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "body")
            })
            .expect("body element");

        let h1_id = state
            .doc
            .get_children(body_id)
            .into_iter()
            .find(|&id| {
                state
                    .doc
                    .node(id)
                    .and_then(|n| n.as_element())
                    .is_some_and(|el| el.tag_name() == "h1")
            })
            .expect("h1 element");

        let body_style = state.styles.get(body_id).expect("body style");
        assert_eq!(
            body_style.background_color,
            Color::rgb(0x34, 0x49, 0x5e)
        );
        assert_eq!(body_style.max_width, Some(480.0)); // 30em * 16px = 480px
        assert_eq!(body_style.text_align, TextAlign::Center);

        let h1_style = state.styles.get(h1_id).expect("h1 style");
        assert_eq!(h1_style.color, Color::rgb(0xe7, 0x4c, 0x3c));
        assert_eq!(h1_style.font_size, 32.0); // 2em * parent (16px) = 32px
        assert_eq!(h1_style.text_align, TextAlign::Center); // inherited from body

        // Cleanup
        let _ = fs::remove_file(css_file);
        let _ = fs::remove_file(html_file);
        let _ = fs::remove_dir(temp_dir);
    }

    #[test]
    fn test_find_image_elements_in_document() {
        let html = r#"<html>
            <body>
                <img src="first.png" />
                <div><img src="  second.jpg  " /></div>
                <img /> <!-- No src -->
            </body>
        </html>"#;
        let doc = parse_html(html);
        let images = find_image_elements(&doc);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].0, "first.png");
        assert_eq!(images[1].0, "second.jpg");
    }

    #[test]
    fn test_shell_image_loading_and_e2e_render() {
        use std::fs;
        let temp_dir = std::env::temp_dir().join(format!("lilies_img_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        // Create a 2x2 red PNG
        let mut img = image::RgbaImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgba([255, 0, 0, 255]);
        }
        let img_path = temp_dir.join("red.png");
        let mut cursor = std::io::Cursor::new(Vec::new());
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        fs::write(&img_path, cursor.into_inner()).expect("write png");

        let html_content = r#"<html>
            <body style="background-color: #000000; margin: 0px; padding: 0px;">
                <img src="red.png" width="40" height="40" style="margin: 0px;" />
            </body>
        </html>"#;
        let html_path = temp_dir.join("index.html");
        fs::write(&html_path, html_content).expect("write html");

        let target_url = format!("file://{}", html_path.display());
        let state = ShellState::new(target_url);

        let mut scroll = 0.0;
        let mut total_h = 0.0;
        let (window_pixmap, lbox) = render_window_pixmap_interactive_with_cache(
            &state.doc,
            state.html_id,
            &state.styles,
            200,
            200,
            &state.current_url,
            &mut scroll,
            &mut total_h,
            false,
            &state.current_url,
            0,
            &state.image_cache,
        )
        .expect("render window pixmap");

        assert!(lbox.dimensions.content.width > 0.0);

        // Top toolbar is 36px high. At y = 36..76 and x = 0..40, pixels should be red!
        let pixel = window_pixmap.get_pixel(10, TOOLBAR_HEIGHT + 10).unwrap();
        assert_eq!(pixel, Color::rgb(255, 0, 0));

        // Outside image (e.g. x = 60, y = 36 + 10) should be body background (black)
        let bg_pixel = window_pixmap.get_pixel(60, TOOLBAR_HEIGHT + 10).unwrap();
        assert_eq!(bg_pixel, Color::BLACK);

        let _ = fs::remove_file(img_path);
        let _ = fs::remove_file(html_path);
        let _ = fs::remove_dir(temp_dir);
    }

    #[test]
    fn test_milestone10_demo_page_render_with_aspect_ratio_and_transparency() {
        let doc = parse_html(IMAGE_DEMO_HTML);
        let styles = compute_styles_for_document(&doc);
        let html_id = find_html_root(&doc).unwrap();

        let image_cache = ImageCache::new();
        // Insert a 200x100 half-transparent green image for the Rust logo URL
        let mut pixels = Vec::with_capacity(200 * 100 * 4);
        for _ in 0..(200 * 100) {
            pixels.extend_from_slice(&[0, 255, 0, 128]);
        }
        let decoded = std::sync::Arc::new(lilies_net::DecodedImage::new(200, 100, pixels));
        image_cache.insert(
            "https://upload.wikimedia.org/wikipedia/commons/d/d5/Rust_programming_language_black_logo.png",
            decoded,
        );

        let (pixmap, total_h, _root_box) = render_webpage_with_images(
            &doc,
            html_id,
            &styles,
            800,
            600,
            0.0,
            &image_cache,
        )
        .expect("rendered webpage");

        assert!(total_h > 100.0);
        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);
    }

    #[test]
    fn test_milestone11_flex_demo_page_render_e2e() {
        let (html, url) = fetch_or_fallback("about:flex");
        assert_eq!(url, "about:flex");
        assert!(html.contains("display: flex"));

        let doc = parse_html(&html);
        let styles = compute_styles_for_document(&doc);
        let html_id = find_html_root(&doc).unwrap();

        let (pixmap, total_h) = render_webpage_to_pixmap(
            &doc,
            html_id,
            &styles,
            800,
            600,
            0.0,
        )
        .expect("rendered flex webpage");

        assert!(total_h > 100.0);
        assert_eq!(pixmap.width, 800);
        assert_eq!(pixmap.height, 600);

        // Body background color #0f172a (r=15, g=23, b=42)
        let body_pixel = pixmap.get_pixel(10, 10).expect("body pixel");
        assert_eq!(body_pixel.r, 0x0f);
        assert_eq!(body_pixel.g, 0x17);
        assert_eq!(body_pixel.b, 0x2a);

        // Navbar background color #1e293b (r=30, g=41, b=59)
        // Body padding is 20px, navbar padding is 16px
        let nav_pixel = pixmap.get_pixel(100, 30).expect("nav pixel");
        assert_eq!(nav_pixel.r, 0x1e);
        assert_eq!(nav_pixel.g, 0x29);
        assert_eq!(nav_pixel.b, 0x3b);
    }

    #[test]
    fn test_default_startup_welcome_page() {
        let (html, url) = fetch_or_fallback("about:welcome");
        assert_eq!(url, "about:welcome");
        assert_eq!(html, DEFAULT_HTML);

        let (empty_html, empty_url) = fetch_or_fallback("");
        assert_eq!(empty_url, "about:welcome");
        assert_eq!(empty_html, DEFAULT_HTML);

        let (blank_html, blank_url) = fetch_or_fallback("about:blank");
        assert_eq!(blank_url, "about:blank");
        assert!(blank_html.contains("body"));

        let state = ShellState::new("about:welcome".to_string());
        assert_eq!(state.current_url, "about:welcome");
        assert_eq!(state.address_input_buffer, "about:welcome");
        assert!(state.doc.arena().len() > 1);
    }

    #[test]
    fn test_address_bar_typing_fast_cache_hit() {
        let mut state = ShellState::new("about:welcome".to_string());
        assert!(state.cached_page.is_none());
        assert!(state.cached_window.is_none());

        // First frame: renders page from scratch and populates cache
        let (initial_window_webpage_pixel, width, height) = {
            let frame = state.render_window_frame(800, 600).expect("first frame");
            (
                frame.get_pixel(400, TOOLBAR_HEIGHT + 100),
                frame.width,
                frame.height,
            )
        };
        assert_eq!(width, 800);
        assert_eq!(height, 600);
        assert!(state.cached_page.is_some());
        assert!(state.cached_window.is_some());

        let initial_page_pixel = state.cached_page.as_ref().unwrap().pixmap.get_pixel(400, 100);
        assert_eq!(initial_page_pixel, initial_window_webpage_pixel);

        // User focuses address bar and types
        state.is_address_bar_focused = true;
        state.address_input_buffer = "hello".to_string();
        state.cursor_position = 5;

        // Second frame: should be a fast cache hit!
        let second_pixel = state
            .render_window_frame(800, 600)
            .expect("second frame")
            .get_pixel(400, TOOLBAR_HEIGHT + 100);

        // The cached_page should still be intact and untouched
        assert_eq!(
            state.cached_page.as_ref().unwrap().pixmap.get_pixel(400, 100),
            initial_page_pixel
        );
        // The webpage area below TOOLBAR_HEIGHT must remain identical
        assert_eq!(second_pixel, initial_window_webpage_pixel);

        // Third frame: typing more characters
        state.handle_char_input(" world");
        assert_eq!(state.address_input_buffer, "hello world");
        let third_pixel = state
            .render_window_frame(800, 600)
            .expect("third frame")
            .get_pixel(400, TOOLBAR_HEIGHT + 100);
        assert_eq!(third_pixel, initial_window_webpage_pixel);

        // Resizing window invalidates cached page and re-renders
        let (resized_w, resized_h) = {
            let resized_frame = state.render_window_frame(600, 500).expect("resized frame");
            (resized_frame.width, resized_frame.height)
        };
        assert_eq!(resized_w, 600);
        assert_eq!(resized_h, 500);
        assert_eq!(state.cached_page.as_ref().unwrap().width, 600);
        assert_eq!(
            state.cached_page.as_ref().unwrap().page_height,
            500 - TOOLBAR_HEIGHT
        );

        // Navigating to new URL invalidates both caches
        state.navigate("about:blank", true);
        assert!(state.cached_page.is_none());
        assert!(state.cached_window.is_none());
        assert_eq!(state.current_url, "about:blank");
    }

    #[test]
    fn test_milestone12_about_js_demo_page_e2e() {
        let mut state = ShellState::new("about:js".to_string());
        assert_eq!(state.current_url, "about:js");

        // Verify initial script executed during page load:
        // Heading should have been changed from "Static Heading" to "Dynamic Heading (Set by JS)"
        let heading_id = state
            .js_runtime
            .find_element_by_id("heading")
            .expect("heading node must exist");
        assert_eq!(
            state.doc.text_content(heading_id),
            "Dynamic Heading (Set by JS)"
        );

        let counter_id = state
            .js_runtime
            .find_element_by_id("counter-text")
            .expect("counter node must exist");
        assert_eq!(state.doc.text_content(counter_id), "Count: 0");

        // Initial render
        let frame1 = state.render_window_frame(800, 600).expect("initial frame");
        assert_eq!(frame1.width, 800);
        assert_eq!(frame1.height, 600);

        // Click button once
        let btn_id = state
            .js_runtime
            .find_element_by_id("btn")
            .expect("btn node must exist");
        state
            .js_runtime
            .dispatch_click(btn_id)
            .expect("dispatch click failed");

        assert!(state.js_runtime.take_dirty(), "DOM should be dirty after click");
        state.doc = state.js_runtime.doc().clone();
        assert_eq!(state.doc.text_content(counter_id), "Count: 1");

        state.styles = load_styles_with_external(&state.doc, &state.current_url);
        state.cached_page = None;
        state.cached_window = None;

        // Render frame after click
        let frame2 = state.render_window_frame(800, 600).expect("frame after click");
        assert_eq!(frame2.width, 800);

        // Click button a second time
        state
            .js_runtime
            .dispatch_click(btn_id)
            .expect("second click failed");
        assert!(state.js_runtime.take_dirty());
        state.doc = state.js_runtime.doc().clone();
        assert_eq!(state.doc.text_content(counter_id), "Count: 2");
    }

    #[test]
    fn test_milestone12_external_script_loading() {
        let temp_dir = std::env::temp_dir().join(format!("lilies_test_js_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let js_path = temp_dir.join("script.js");
        let html_path = temp_dir.join("index.html");

        fs::write(&js_path, "var el = document.getElementById('target'); el.textContent = 'Loaded from External';").unwrap();
        let html_content = format!(
            r#"<html><body><h2 id="target">Initial</h2><script src="{}"></script></body></html>"#,
            js_path.to_str().unwrap()
        );
        fs::write(&html_path, html_content).unwrap();

        let target_url = format!("file://{}", html_path.to_str().unwrap());
        let state = ShellState::new(target_url);

        let target_id = state
            .js_runtime
            .find_element_by_id("target")
            .expect("target must exist");
        assert_eq!(state.doc.text_content(target_id), "Loaded from External");

        let _ = fs::remove_file(js_path);
        let _ = fs::remove_file(html_path);
        let _ = fs::remove_dir(temp_dir);
    }
}
