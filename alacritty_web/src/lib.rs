//! Alacritty Web - WASM terminal renderer.

mod renderer;
pub mod terminal;
mod websocket;

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// Font configuration for the terminal renderer, matching `FontConfig` in `canvas2d.rs`.
///
/// All fields have sensible defaults so the struct can be constructed with just
/// `FontConfig::default()`.
pub use renderer::canvas2d::FontConfig;

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
    ws: Option<websocket::WsConnection>,
    /// Data fed directly (replay/WebContainer), not via WebSocket.
    local_data: Vec<Vec<u8>>,
}

/// The main Alacritty terminal component for the browser.
#[wasm_bindgen]
pub struct AlacrittyTerminal {
    state: Rc<RefCell<AppState>>,
    #[allow(dead_code)]
    canvas: HtmlCanvasElement,
}

#[wasm_bindgen]
impl AlacrittyTerminal {
    /// Create a new terminal attached to the given canvas element.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<AlacrittyTerminal, JsError> {
        init_wasm();
        log::info!("Initializing AlacrittyTerminal");

        let renderer = renderer::canvas2d::Canvas2dRenderer::new(&canvas)?;
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
            ws: None,
            local_data: Vec::new(),
        }));

        let term = AlacrittyTerminal {
            state,
            canvas,
        };

        term.start_render_loop();

        Ok(term)
    }

    /// Connect to a WebSocket PTY server.
    pub fn connect(&mut self, ws_url: &str) -> Result<(), JsError> {
        let ws = websocket::WsConnection::new(ws_url)?;
        self.state.borrow_mut().ws = Some(ws);
        Ok(())
    }

    /// Disconnect from the PTY server.
    pub fn disconnect(&mut self) {
        self.state.borrow_mut().ws = None;
    }

    /// Feed data directly into the terminal (for replay/local input, no PTY).
    pub fn feed(&self, data: &[u8]) {
        if let Ok(mut app) = self.state.try_borrow_mut() {
            app.local_data.push(data.to_vec());
            app.dirty = true;
        }
    }

    /// Write data to the PTY (send input).
    pub fn write(&self, data: &[u8]) {
        if let Ok(mut app) = self.state.try_borrow_mut() {
            if let Some(ws) = &mut app.ws {
                ws.send_pty_data(data);
            }
        }
    }

    /// Send a resize message to the server.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let mut app = self.state.borrow_mut();
        app.terminal.resize(cols, rows);
        app.dirty = true;
        if let Some(ws) = &mut app.ws {
            ws.send_resize(cols, rows, 0, 0);
        }
    }

    /// Get cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        self.state.borrow().renderer.cell_width()
    }

    /// Get cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        self.state.borrow().renderer.cell_height()
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

    /// Set the line height multiplier and trigger a re-render.
    pub fn set_line_height_multiplier(&self, multiplier: f32) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_line_height_multiplier(multiplier);
        app.dirty = true;
    }

    /// Get the number of columns in the terminal grid.
    pub fn cols(&self) -> u16 {
        self.state.borrow().terminal.cols()
    }

    /// Get the number of rows in the terminal grid.
    pub fn rows(&self) -> u16 {
        self.state.borrow().terminal.rows()
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
        let callback = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
        let callback_clone = callback.clone();

        *callback.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            // All data processing and rendering in a single borrow.
            if let Ok(mut app) = state.try_borrow_mut() {
                // Flush pending outgoing messages once the connection is open.
                if let Some(ws) = &mut app.ws {
                    ws.flush_pending();
                }

                // Drain WebSocket data (polled from JS-side queue, no WASM callbacks).
                if let Some(ws) = &app.ws {
                    let ws_chunks = ws.drain_incoming();
                    if !ws_chunks.is_empty() {
                        for chunk in ws_chunks {
                            app.terminal.process_bytes(&chunk);
                        }
                        app.dirty = true;
                    }
                }

                // Drain locally-fed data.
                if !app.local_data.is_empty() {
                    let local: Vec<Vec<u8>> = app.local_data.drain(..).collect();
                    for chunk in local {
                        app.terminal.process_bytes(&chunk);
                    }
                    app.dirty = true;
                }

                // Render if dirty.
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
                    if let Err(e) = win.request_animation_frame(cb.as_ref().unchecked_ref()) {
                        log::warn!("request_animation_frame failed: {e:?}");
                    }
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
