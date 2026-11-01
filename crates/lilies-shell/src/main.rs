#![deny(unsafe_code)]

use lilies_css::StyleTree;
use lilies_dom::{Document, NodeId};
use lilies_html::{load_styles_with_external, parse_html};
use lilies_layout::{
    build_layout_tree, find_parent_link, hit_test, layout, Dimensions, EdgeSizes, LayoutBox, Rect,
};
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
    let mut root_box = build_layout_tree(html_id, doc.arena(), styles)?;
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
    let mut window_pixmap = Pixmap::new(width, height, Color::rgb(0x18, 0x18, 0x25));

    let page_height = height.saturating_sub(TOOLBAR_HEIGHT);
    let layout_box = if page_height > 0 && width > 0 {
        let (page_pixmap, content_h, lbox) =
            render_webpage_with_layout(doc, html_id, styles, width, page_height, *scroll_offset_y)?;
        *total_content_height = content_h;
        let max_scroll = (content_h - page_height as f32).max(0.0);
        *scroll_offset_y = scroll_offset_y.clamp(0.0, max_scroll);

        window_pixmap.blit(&page_pixmap, 0, TOOLBAR_HEIGHT);
        lbox
    } else {
        *total_content_height = 0.0;
        *scroll_offset_y = 0.0;
        let mut empty_box = build_layout_tree(html_id, doc.arena(), styles)?;
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
    if trimmed.is_empty() {
        return "https://example.com".to_string();
    }

    if trimmed.contains("://") || trimmed.starts_with("file:") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

/// Shell state holding browser DOM, styles, navigation history, and interactive UI state.
pub struct ShellState {
    pub current_url: String,
    pub doc: Document,
    pub styles: StyleTree,
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
}

impl ShellState {
    pub fn new(initial_target: String) -> Self {
        let (html_content, current_url) = fetch_or_fallback(&initial_target);
        let doc = parse_html(&html_content);
        let styles = load_styles_with_external(&doc, &current_url);
        let html_id = find_html_root(&doc).expect("Root <html> element required in document");

        Self {
            address_input_buffer: current_url.clone(),
            cursor_position: current_url.len(),
            history: vec![current_url.clone()],
            history_index: 0,
            current_url,
            doc,
            styles,
            html_id,
            current_layout_box: None,
            scroll_offset_y: 0.0,
            total_content_height: 0.0,
            is_address_bar_focused: false,
            mouse_pos: (0.0, 0.0),
            alt_pressed: false,
        }
    }

    pub fn navigate(&mut self, target_url: &str, push_history: bool) {
        println!("Navigating to: {}", target_url);
        let (html_content, loaded_url) = fetch_or_fallback(target_url);
        let doc = parse_html(&html_content);
        let styles = load_styles_with_external(&doc, &loaded_url);
        let Some(html_id) = find_html_root(&doc) else {
            eprintln!("Document missing <html> root element");
            return;
        };

        self.doc = doc;
        self.styles = styles;
        self.html_id = html_id;
        self.current_url = loaded_url;
        self.address_input_buffer = self.current_url.clone();
        self.cursor_position = self.address_input_buffer.len();
        self.scroll_offset_y = 0.0;
        self.is_address_bar_focused = false;

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
}

fn fetch_or_fallback(target: &str) -> (String, String) {
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

    let (pixmap, layout_box) = render_window_pixmap_interactive(
        &state.doc,
        state.html_id,
        &state.styles,
        width,
        height,
        &state.current_url,
        &mut state.scroll_offset_y,
        &mut state.total_content_height,
        state.is_address_bar_focused,
        &state.address_input_buffer,
        state.cursor_position,
    )
    .ok_or("Failed to build layout tree from root")?;

    state.current_layout_box = Some(layout_box);

    let mut buffer = surface.buffer_mut()?;
    copy_pixmap_to_softbuffer(&pixmap, &mut buffer);
    buffer.present()?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lilies: Modern Browser Engine (Milestone 8 - Interactive Navigation) ===");

    // Step 1: Parse CLI argument or default to https://example.com
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://example.com".to_string());
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
    use lilies_css::{
        compute_styles, compute_styles_for_document, parse_styles_from_document, StyleSheet,
    };

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
        assert_eq!(normalize_address_input("   "), "https://example.com");
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
}
