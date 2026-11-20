//! The immediate-mode UI toolkit: input model, widget state and the drawing
//! entry points for the reader chrome.
//!
//! The UI is drawn into a plain RGBA [`canvas::Bitmap`]; platform shells only
//! pump events in and blit the finished frame out. This keeps 100% of the
//! visual code shared between Windows and Linux (and testable headlessly).

pub mod canvas;
pub mod font;
pub mod icons;
pub mod theme;
pub mod widgets;

use crate::geom::{Point, Rect};

pub use canvas::{Bitmap, Color};
pub use font::Font;
pub use theme::{Metrics, Theme};

/// Stable widget identifier (FNV-1a of a caller supplied label).
pub type Id = u64;

pub fn id(label: &str) -> Id {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in label.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Logical keys the UI understands. Printable characters arrive as
/// [`Key::Char`]; modifiers are carried on [`Input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Space,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub key: Key,
    /// True when the OS generated a key-repeat.
    pub repeat: bool,
}

/// A snapshot of platform input for one frame.
#[derive(Debug, Clone, Default)]
pub struct Input {
    pub mouse: Point,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub double_click: bool,
    pub right_pressed: bool,
    pub wheel: f32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub keys: Vec<KeyEvent>,
    /// Printable characters typed this frame.
    pub text: String,
    /// Second-scale timestamp (monotonic).
    pub time: f64,
    /// Text available from the system clipboard (Ctrl+V).
    pub paste: Option<String>,
}

impl Input {
    pub fn key_pressed(&self, key: Key) -> bool {
        self.keys.iter().any(|e| e.key == key && !e.repeat)
    }

    pub fn key_down_any(&self, key: Key) -> bool {
        self.keys.iter().any(|e| e.key == key)
    }

    pub fn any_key(&self) -> bool {
        !self.keys.is_empty()
    }

    pub fn ctrl_key(&self, key: Key) -> bool {
        self.ctrl && self.key_pressed(key)
    }

    pub fn clear_transient(&mut self) {
        self.keys.clear();
        self.text.clear();
        self.pressed = false;
        self.released = false;
        self.right_pressed = false;
        self.double_click = false;
        self.wheel = 0.0;
        self.paste = None;
    }
}

#[derive(Debug, Default)]
struct DragState {
    id: Id,
    start: Point,
    last: Point,
}

/// Interaction state that must survive between frames.
#[derive(Debug, Default)]
pub struct UiState {
    pub hot: Option<Id>,
    pub active: Option<Id>,
    pub focus: Option<Id>,
    hover_since: Option<(Id, f64)>,
    drag: Option<DragState>,
    /// Set while a modal dialog is open; suppresses interaction underneath.
    pub modal: bool,
    /// Tooltip requested during this frame (drawn last, on top of everything).
    pub tooltip: Option<(String, Point)>,
    /// Text copied by a widget this frame (platform puts it on the clipboard).
    pub clipboard_set: Option<String>,
    /// Animation clock used by the loading spinner.
    pub start_time: f64,
}

impl UiState {
    pub fn begin_frame(&mut self) {
        self.tooltip = None;
        self.clipboard_set = None;
    }

    pub fn dragging(&self) -> Option<Id> {
        self.drag.as_ref().map(|d| d.id)
    }

    pub fn drag_delta(&self) -> Point {
        match &self.drag {
            Some(d) => Point::new(d.last.x - d.start.x, d.last.y - d.start.y),
            None => Point::ZERO,
        }
    }

    pub fn drag_origin(&self) -> Point {
        self.drag.as_ref().map(|d| d.start).unwrap_or(Point::ZERO)
    }
}

/// Draws the chrome. Holds the frame's canvas, theme, font and input.
pub struct Ui<'a> {
    pub bmp: &'a mut Bitmap,
    pub font: &'a Font,
    pub theme: Theme,
    pub input: &'a Input,
    pub state: &'a mut UiState,
    /// Display scale factor (1.0 = 96 DPI). Reserved for high-DPI shells.
    pub input_scale: f32,
}

impl<'a> Ui<'a> {
    pub fn new(
        bmp: &'a mut Bitmap,
        font: &'a Font,
        theme: Theme,
        input: &'a Input,
        state: &'a mut UiState,
    ) -> Self {
        Ui {
            bmp,
            font,
            theme,
            input,
            state,
            input_scale: 1.0,
        }
    }

    pub fn scale(&self) -> f32 {
        self.input_scale
    }

    /// True when the pointer is inside `r` and interactions are not blocked.
    pub fn hovered(&self, r: Rect) -> bool {
        !self.state.modal && r.contains(self.input.mouse)
    }

    /// Handles hover/press/release bookkeeping and reports a click.
    pub fn interact(&mut self, id: Id, r: Rect, enabled: bool) -> bool {
        if !enabled {
            if self.state.hot == Some(id) {
                self.state.hot = None;
            }
            return false;
        }
        if self.state.modal {
            return false;
        }
        let inside = r.contains(self.input.mouse);
        if inside {
            self.state.hot = Some(id);
        } else if self.state.hot == Some(id) {
            self.state.hot = None;
        }
        if inside && self.input.pressed {
            self.state.active = Some(id);
            self.state.focus = Some(id);
            self.state.drag = Some(DragState {
                id,
                start: self.input.mouse,
                last: self.input.mouse,
            });
        }
        let mut clicked = false;
        if let Some(drag) = &mut self.state.drag {
            if drag.id == id {
                drag.last = self.input.mouse;
            }
        }
        if self.input.released {
            if self.state.active == Some(id) {
                clicked = inside;
                self.state.active = None;
            }
            if self.state.drag.as_ref().map(|d| d.id) == Some(id) {
                self.state.drag = None;
            }
        }
        clicked
    }

    /// True while `id` is being dragged and the pointer has moved.
    pub fn is_dragging(&self, id: Id) -> bool {
        self.state.dragging() == Some(id) && self.input.down
    }

    pub fn request_tooltip(&mut self, text: &str, at: Point) {
        if self.state.tooltip.is_none() && !text.is_empty() {
            self.state.tooltip = Some((text.to_string(), at));
        }
    }

    /// Draws the pending tooltip (call once, after the chrome).
    pub fn draw_tooltip(&mut self) {
        let Some((text, at)) = self.state.tooltip.clone() else {
            return;
        };
        let pad = 7.0;
        let w = self.font.text_width(&text, Metrics::SMALL_TEXT_SIZE) + pad * 2.0;
        let h = 20.0;
        let mut x = at.x + 14.0;
        let mut y = at.y + 18.0;
        if x + w > self.bmp.width as f32 {
            x = self.bmp.width as f32 - w - 4.0;
        }
        if y + h > self.bmp.height as f32 {
            y = at.y - h - 8.0;
        }
        let r = Rect::new(x, y, w, h);
        self.bmp.drop_shadow(r, 4.0, 0.5, Color::rgba(0, 0, 0, 0x60));
        self.bmp.fill_round_rect(r, 4.0, self.theme.tooltip_bg);
        let ty = r.y + (r.h + self.font.ascent(Metrics::SMALL_TEXT_SIZE) * 0.72) * 0.5;
        self.font.draw_text(
            self.bmp,
            r.x + pad,
            ty,
            Metrics::SMALL_TEXT_SIZE,
            self.theme.tooltip_text,
            &text,
        );
    }

    /// Convenience: draw a divider line.
    pub fn divider_h(&mut self, r: Rect) {
        self.bmp
            .fill_rect(Rect::new(r.x, r.y, r.w, 1.0), self.theme.divider);
    }

    pub fn divider_v(&mut self, r: Rect) {
        self.bmp
            .fill_rect(Rect::new(r.x, r.y, 1.0, r.h), self.theme.divider);
    }

    /// Rotating loading indicator.
    pub fn spinner(&mut self, center: Point, radius: f32) {
        let t = self.input.time;
        let steps = 10;
        for i in 0..steps {
            let a = (t * 3.0 + i as f64 * 0.28) as f32;
            let alpha = (i as f32 / steps as f32 * 0.85 + 0.15).min(1.0);
            let xx = center.x + radius * a.cos();
            let yy = center.y + radius * a.sin();
            self.bmp
                .fill_circle(xx, yy, 1.4, self.theme.accent.fade(alpha));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(id("open"), id("open"));
        assert_ne!(id("open"), id("close"));
    }

    #[test]
    fn key_helpers_see_events() {
        let mut input = Input {
            ctrl: true,
            ..Default::default()
        };
        input.keys.push(KeyEvent {
            key: Key::Char('o'),
            repeat: false,
        });
        assert!(input.key_pressed(Key::Char('o')));
        assert!(input.ctrl_key(Key::Char('o')));
        assert!(!input.key_pressed(Key::Char('p')));
        input.clear_transient();
        assert!(input.keys.is_empty());
    }
}
