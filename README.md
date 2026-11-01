# lilies

> **L**ightweight **I**nline **L**ayout and **I**nterface **E**ngine **S**ystem

`lilies` is an independent, custom web browser rendering engine and interactive desktop browser application written in Rust from scratch. Every workspace crate strictly enforces `#![deny(unsafe_code)]`.

---

## Workspace Architecture

`lilies` is organized as a modular Cargo workspace composed of specialized crates:

| Crate | Description |
|---|---|
| [`lilies-dom`](crates/lilies-dom) | Arena-allocated DOM tree (`Node`, `Element`, `Text`, `Document`) with cycle prevention and mutation helpers. |
| [`lilies-html`](crates/lilies-html) | WHATWG HTML tokenizer and tree builder, paragraph auto-closing, `<style>` extraction, and external `<link rel="stylesheet">` discovery. |
| [`lilies-css`](crates/lilies-css) | CSS parser, selector matching, specificity calculation, cascade resolution, inheritance, and unit resolution (`px`, `em`, `rem`). |
| [`lilies-layout`](crates/lilies-layout) | Box model layout engine (Block Formatting Context, inline wrapping via `fontdue`, `margin: auto` centering, `max-width`, `text-align`, hit-testing). |
| [`lilies-paint`](crates/lilies-paint) | Display list compilation, software rasterizer with alpha blending, glyph rasterization, and pixel buffer export. |
| [`lilies-net`](crates/lilies-net) | Network layer supporting HTTP/HTTPS fetching with TLS via `ureq`, custom User-Agent, and local `file://` scheme support. |
| [`lilies-shell`](crates/lilies-shell) | Interactive desktop browser shell (`winit` + `softbuffer`) with an editable address bar, link clicking, mouse scrolling, and navigation history. |

---

## Prerequisites

- [Rust](https://rustup.rs/) (2021 edition, stable toolchain)
- Standard Linux windowing development libraries (for `winit` / `softbuffer`, e.g. `libxkbcommon`, `fontconfig`)

---

## How to Run

### 1. Launch Interactive Browser with Default Welcome Page

```bash
cargo run -p lilies-shell
```

### 2. Open a Remote Website (HTTP / HTTPS)

```bash
cargo run -p lilies-shell -- https://example.com
```

### 3. Open a Local HTML File

```bash
cargo run -p lilies-shell -- file://$(pwd)/index.html
```

---

## Interactive Controls & Navigation

- **Address Bar**: Click the address bar at the top of the window to focus it. Type any URL or path and press `Enter` to navigate.
- **Clickable Links**: Hovering over hyperlinks (`<a href="...">`) changes the cursor to a pointer. Clicking the link will resolve relative or absolute URLs and navigate to the target page.
- **Back Navigation**: Press `Alt + Left Arrow` (or `Backspace` when the address bar is not focused) to navigate backward in your browsing history.
- **Scrolling**: Use the mouse wheel or touchpad scroll gesture to scroll vertically through page content.

---

## Testing & Quality Assurance

All workspace crates strictly deny unsafe code and compile cleanly without warnings:

```bash
# Run unit and integration tests across the workspace
cargo test --workspace

# Run Clippy lint checks
cargo clippy --workspace --all-targets -- -D warnings
```

---

## License

Dual-licensed under MIT or Apache-2.0.
