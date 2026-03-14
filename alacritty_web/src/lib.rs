//! Alacritty Web - WASM terminal renderer.

mod renderer;
pub mod terminal;
mod websocket;

use std::cell::RefCell;
use std::rc::Rc;

use serde::Deserialize;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// Configuration passed from JavaScript.
///
/// Accepted as a JS object:
/// ```js
/// { fontSize: 14, fontFamily: "monospace", theme: "dark" }
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TerminalConfig {
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default = "default_font_family")]
    font_family: String,
    #[serde(default)]
    theme: Option<String>,
}

fn default_font_size() -> f32 {
    14.0
}

fn default_font_family() -> String {
    "'Fira Code', 'Cascadia Code', 'Source Code Pro', monospace".to_string()
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_family: default_font_family(),
            theme: None,
        }
    }
}

/// Initialize panic hook and logger for better WASM debugging.
fn init_wasm() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
}

/// Shared state for the terminal + renderer.
struct AppState {
    renderer: renderer::canvas2d::Canvas2dRenderer,
    terminal: terminal::WebTerminal,
    dirty: bool,
}

/// The main Alacritty terminal component for the browser.
#[wasm_bindgen]
pub struct AlacrittyTerminal {
    state: Rc<RefCell<AppState>>,
    /// Incoming PTY data is queued here to avoid RefCell borrow conflicts.
    incoming_data: Rc<RefCell<Vec<Vec<u8>>>>,
    ws: Option<websocket::WsConnection>,
    canvas: HtmlCanvasElement,
}

#[wasm_bindgen]
impl AlacrittyTerminal {
    /// Create a new terminal attached to the given canvas element.
    ///
    /// The optional `config` parameter accepts a JS object:
    /// ```js
    /// { fontSize: 14, fontFamily: "monospace", theme: "dark" }
    /// ```
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement, config: JsValue) -> Result<AlacrittyTerminal, JsError> {
        init_wasm();
        log::info!("Initializing AlacrittyTerminal");

        let cfg: TerminalConfig = if config.is_undefined() || config.is_null() {
            TerminalConfig::default()
        } else {
            serde_wasm_bindgen::from_value(config)
                .map_err(|e| JsError::new(&format!("Invalid config: {e}")))?
        };

        log::info!("Config: {cfg:?}");

        let mut renderer = renderer::canvas2d::Canvas2dRenderer::new(&canvas)?;

        // Apply config to renderer.
        renderer.set_font_size(cfg.font_size);
        renderer.set_font_family(&cfg.font_family);

        let cell_w = renderer.cell_width();
        let cell_h = renderer.cell_height();

        let css_width = canvas.client_width().max(1) as f32;
        let css_height = canvas.client_height().max(1) as f32;
        let cols = (css_width / cell_w).floor().max(1.0) as u16;
        let rows = (css_height / cell_h).floor().max(1.0) as u16;

        log::info!("Initial grid: {cols}x{rows}");

        let terminal = terminal::WebTerminal::new(cols, rows);

        let state = Rc::new(RefCell::new(AppState {
            renderer,
            terminal,
            dirty: true,
        }));

        let incoming_data: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));

        let term = AlacrittyTerminal {
            state,
            incoming_data,
            ws: None,
            canvas,
        };

        term.start_render_loop();

        Ok(term)
    }

    /// Connect to a WebSocket PTY server.
    pub fn connect(&mut self, ws_url: &str) -> Result<(), JsError> {
        // WebSocket data goes into the queue, not directly into the terminal.
        let queue = self.incoming_data.clone();
        let ws = websocket::WsConnection::new(ws_url, move |data| {
            queue.borrow_mut().push(data.to_vec());
        })?;
        self.ws = Some(ws);
        Ok(())
    }

    /// Disconnect from the PTY server.
    pub fn disconnect(&mut self) {
        self.ws = None;
    }

    /// Write data to the PTY (send input).
    pub fn write(&self, data: &[u8]) {
        if let Some(ws) = &self.ws {
            ws.send_pty_data(data);
        }
    }

    /// Send a resize message to the server and update the terminal grid.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.state.borrow_mut().terminal.resize(cols, rows);
        self.state.borrow_mut().dirty = true;
        if let Some(ws) = &self.ws {
            ws.send_resize(cols, rows, 0, 0);
        }
    }

    /// Focus the canvas element.
    pub fn focus(&self) {
        let html_element: &web_sys::HtmlElement = self.canvas.as_ref();
        let _ = html_element.focus();
    }

    /// Set the font size in pixels and trigger a re-render.
    pub fn set_font_size(&self, size_px: f32) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_font_size(size_px);
        app.dirty = true;
    }

    /// Set the font family and trigger a re-render.
    pub fn set_font_family(&self, family: &str) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_font_family(family);
        app.dirty = true;
    }

    /// Get cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        self.state.borrow().renderer.cell_width()
    }

    /// Get cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        self.state.borrow().renderer.cell_height()
    }

    /// Get the number of columns in the terminal grid.
    pub fn cols(&self) -> u16 {
        self.state.borrow().terminal.cols()
    }

    /// Get the number of rows in the terminal grid.
    pub fn rows(&self) -> u16 {
        self.state.borrow().terminal.rows()
    }

    /// Feed raw bytes into the terminal as if received from a PTY.
    ///
    /// This is useful for demos, replays, or custom data sources that
    /// bypass the WebSocket connection.
    pub fn feed(&self, data: &[u8]) {
        self.incoming_data.borrow_mut().push(data.to_vec());
    }

    /// Clean up resources.
    pub fn dispose(self) {
        drop(self);
    }
}

impl AlacrittyTerminal {
    /// Start the requestAnimationFrame render loop.
    fn start_render_loop(&self) {
        let state = self.state.clone();
        let incoming = self.incoming_data.clone();
        let callback = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
        let callback_clone = callback.clone();

        *callback.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            // Drain incoming data into the terminal.
            let chunks: Vec<Vec<u8>> = incoming.borrow_mut().drain(..).collect();
            if !chunks.is_empty() {
                let mut app = state.borrow_mut();
                for chunk in chunks {
                    app.terminal.process_bytes(&chunk);
                }
                app.dirty = true;
            }

            // Render if dirty.
            {
                let mut app = state.borrow_mut();
                if app.dirty {
                    let term = app.terminal.term().clone();
                    let term_guard = term.lock();
                    app.renderer.render(&term_guard);
                    drop(term_guard);
                    app.dirty = false;
                }
            }

            // Schedule next frame.
            if let Some(win) = web_sys::window() {
                if let Some(cb) = callback_clone.borrow().as_ref() {
                    let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
                }
            }
        }) as Box<dyn FnMut()>));

        if let Some(win) = web_sys::window() {
            if let Some(cb) = callback.borrow().as_ref() {
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }

        std::mem::forget(callback);
    }
}
