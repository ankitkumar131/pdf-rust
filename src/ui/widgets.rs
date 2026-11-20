//! Reusable widgets: buttons, toggles, tabs, list rows, scrollbars and the
//! single-line text field used by the page box and the search bar.

use super::icons::{self, Icon};
use super::{Id, Metrics, Ui};
use crate::geom::{clamp, Point, Rect};

/// Draws a standard push button. Returns true when clicked.
pub fn button(ui: &mut Ui, id: Id, r: Rect, label: &str, enabled: bool, primary: bool) -> bool {
    let mut clicked = ui.interact(id, r, enabled);
    if enabled && ui.state.focus == Some(id) {
        // Reachable with Tab: Enter or Space presses it.
        ui.state.mark_activatable(id);
        clicked |= ui.state.keyboard_activated(id);
    }
    let theme = ui.theme;
    let hovered = ui.hovered(r) && enabled;
    let active = ui.state.active == Some(id);
    let (bg, fg) = if !enabled {
        (theme.control_disabled, theme.text_disabled)
    } else if primary {
        let bg = if active {
            theme.accent.with_alpha(200)
        } else if hovered {
            theme.accent
        } else {
            theme.accent
        };
        (bg, theme.accent_text)
    } else if active {
        (theme.control_active, theme.text)
    } else if hovered {
        (theme.control_hover, theme.text)
    } else {
        (theme.control_bg, theme.text)
    };
    ui.bmp.fill_round_rect(r, Metrics::BUTTON_RADIUS, bg);
    if !primary && enabled {
        ui.bmp
            .stroke_round_rect(r, Metrics::BUTTON_RADIUS, theme.control_border, 1.0);
    }
    if ui.state.focus == Some(id) && enabled {
        ui.bmp.stroke_round_rect(
            r.inset(-1.0),
            Metrics::BUTTON_RADIUS + 1.0,
            theme.accent.fade(0.55),
            1.0,
        );
    }
    let text = ui.font.ellipsize(label, Metrics::TEXT_SIZE, r.w - 12.0);
    ui.font
        .draw_text_centered(ui.bmp, r, Metrics::TEXT_SIZE, fg, &text);
    clicked
}

/// Square icon button, optionally showing a selected (pressed) state.
pub fn icon_button(ui: &mut Ui, id: Id, r: Rect, icon: Icon, enabled: bool, selected: bool) -> bool {
    let clicked = ui.interact(id, r, enabled);
    let theme = ui.theme;
    let hovered = ui.hovered(r) && enabled;
    let active = ui.state.active == Some(id);
    let bg = if !enabled {
        None
    } else if selected {
        Some(theme.accent_soft)
    } else if active {
        Some(theme.control_active)
    } else if hovered {
        Some(theme.control_hover)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.bmp.fill_round_rect(r, Metrics::BUTTON_RADIUS, bg);
    }
    let fg = if !enabled {
        theme.text_disabled
    } else if selected {
        theme.accent
    } else {
        theme.text
    };
    let side = r.w.min(r.h) - 12.0;
    let area = Rect::new(
        r.x + (r.w - side) * 0.5,
        r.y + (r.h - side) * 0.5,
        side,
        side,
    );
    icons::draw(ui.bmp, area, icon, fg);
    if hovered {
        ui.request_tooltip(icon.label(), r.center());
    }
    clicked
}

/// Icon button with a text label underneath/next to it (toolbar "tab" style).
pub fn icon_text_button(
    ui: &mut Ui,
    id: Id,
    r: Rect,
    icon: Icon,
    label: &str,
    enabled: bool,
    selected: bool,
) -> bool {
    let clicked = ui.interact(id, r, enabled);
    let theme = ui.theme;
    let hovered = ui.hovered(r) && enabled;
    let active = ui.state.active == Some(id);
    if let Some(bg) = if !enabled {
        None
    } else if selected {
        Some(theme.accent_soft)
    } else if active {
        Some(theme.control_active)
    } else if hovered {
        Some(theme.control_hover)
    } else {
        None
    } {
        ui.bmp.fill_round_rect(r, Metrics::BUTTON_RADIUS, bg);
    }
    let fg = if !enabled {
        theme.text_disabled
    } else if selected {
        theme.accent
    } else {
        theme.text
    };
    let icon_side = (r.h - 10.0).min(18.0);
    let icon_area = Rect::new(r.x + 7.0, r.y + (r.h - icon_side) * 0.5, icon_side, icon_side);
    icons::draw(ui.bmp, icon_area, icon, fg);
    let text_x = icon_area.right() + 5.0;
    let avail = r.right() - text_x - 7.0;
    let text = ui.font.ellipsize(label, Metrics::TEXT_SIZE, avail);
    let baseline = r.y + (r.h + ui.font.ascent(Metrics::TEXT_SIZE) * 0.72) * 0.5;
    ui.font
        .draw_text(ui.bmp, text_x, baseline, Metrics::TEXT_SIZE, fg, &text);
    if hovered {
        ui.request_tooltip(icon.label(), r.center());
    }
    clicked
}

/// Sidebar tab header.
pub fn tab(ui: &mut Ui, id: Id, r: Rect, label: &str, selected: bool) -> bool {
    let clicked = ui.interact(id, r, true);
    let theme = ui.theme;
    let hovered = ui.hovered(r);
    if hovered && !selected {
        ui.bmp
            .fill_rect(Rect::new(r.x, r.y, r.w, r.h), theme.control_hover.fade(0.6));
    }
    let fg = if selected { theme.text } else { theme.text_muted };
    let text = ui.font.ellipsize(label, Metrics::SMALL_TEXT_SIZE, r.w - 8.0);
    ui.font
        .draw_text_centered(ui.bmp, r, Metrics::SMALL_TEXT_SIZE, fg, &text);
    if selected {
        ui.bmp.fill_rect(
            Rect::new(r.x + 4.0, r.bottom() - 2.0, r.w - 8.0, 2.0),
            theme.accent,
        );
    }
    clicked
}

/// A clickable list row used for thumbnails captions, outline entries and the
/// recent-documents list. Returns true when activated (click or double click).
pub fn list_row(
    ui: &mut Ui,
    id: Id,
    r: Rect,
    title: &str,
    subtitle: Option<&str>,
    selected: bool,
    indent: f32,
) -> bool {
    let clicked = ui.interact(id, r, true);
    let theme = ui.theme;
    let hovered = ui.hovered(r);
    let bg = if selected {
        Some(theme.accent_soft)
    } else if hovered {
        Some(theme.control_hover.fade(0.8))
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.bmp.fill_round_rect(r.inset_xy(2.0, 1.0), 5.0, bg);
    }
    let x = r.x + 8.0 + indent;
    let size = if subtitle.is_some() {
        Metrics::TEXT_SIZE
    } else {
        Metrics::TEXT_SIZE
    };
    let title_fit = ui.font.ellipsize(title, size, r.right() - x - 8.0);
    let baseline = if subtitle.is_some() {
        r.y + r.h * 0.5 - 2.0
    } else {
        r.y + (r.h + ui.font.ascent(size) * 0.7) * 0.5
    };
    let fg = if selected { theme.text } else { theme.text };
    ui.font.draw_text(ui.bmp, x, baseline, size, fg, &title_fit);
    if let Some(sub) = subtitle {
        let sub_fit = ui.font.ellipsize(sub, Metrics::SMALL_TEXT_SIZE, r.right() - x - 8.0);
        ui.font.draw_text(
            ui.bmp,
            x,
            baseline + ui.font.line_height(Metrics::SMALL_TEXT_SIZE) - 1.0,
            Metrics::SMALL_TEXT_SIZE,
            theme.text_muted,
            &sub_fit,
        );
    }
    clicked
}

/// Vertical scrollbar. Returns true when the offset changed this frame.
pub fn scrollbar(
    ui: &mut Ui,
    id: Id,
    track: Rect,
    content_h: f32,
    viewport_h: f32,
    offset: &mut f32,
) -> bool {
    if content_h <= viewport_h + 0.5 || track.h < 8.0 {
        *offset = 0.0;
        return false;
    }
    let max_offset = (content_h - viewport_h).max(0.0);
    if *offset > max_offset {
        *offset = max_offset;
    }
    if *offset < 0.0 {
        *offset = 0.0;
    }
    let thumb_h = (viewport_h / content_h * track.h).clamp(24.0, track.h);
    let travel = track.h - thumb_h;
    let mut changed = false;
    let thumb_y = track.y + travel * (*offset / max_offset).clamp(0.0, 1.0);

    let wide = Rect::new(track.x - 3.0, track.y, track.w + 6.0, track.h);
    let _ = ui.interact(id, wide, true);
    let hovered = ui.hovered(wide) || ui.state.dragging() == Some(id);
    if ui.state.dragging() == Some(id) {
        // The thumb centre follows the pointer while dragging.
        let rel = ((ui.input.mouse.y - track.y - thumb_h * 0.5) / travel.max(1.0)).clamp(0.0, 1.0);
        let new_offset = rel * max_offset;
        if (new_offset - *offset).abs() > 0.01 {
            *offset = new_offset;
            changed = true;
        }
    } else if hovered && ui.input.wheel != 0.0 {
        *offset = (*offset - ui.input.wheel * 60.0).clamp(0.0, max_offset);
        changed = true;
    }
    let color = if hovered {
        ui.theme.scrollbar_hover
    } else {
        ui.theme.scrollbar
    };
    let thumb_rect = Rect::new(track.x + 2.0, thumb_y, track.w - 4.0, thumb_h);
    ui.bmp.fill_round_rect(thumb_rect, thumb_rect.w * 0.5, color);
    changed
}

/// State of a single-line text field.
#[derive(Debug, Clone, Default)]
pub struct TextField {
    pub text: String,
    /// Caret position as a byte offset into `text`.
    pub caret: usize,
    /// Selection anchor (byte offset); `None` when there is no selection.
    pub anchor: Option<usize>,
    pub scroll_x: f32,
}

impl TextField {
    pub fn with_text(text: &str) -> Self {
        TextField {
            text: text.to_string(),
            caret: text.len(),
            anchor: None,
            scroll_x: 0.0,
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|a| {
            if a <= self.caret {
                (a, self.caret)
            } else {
                (self.caret, a)
            }
        })
    }

    fn prev_boundary(&self) -> Option<usize> {
        if self.caret == 0 {
            return None;
        }
        let mut i = self.caret - 1;
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        Some(i)
    }

    fn next_boundary(&self) -> Option<usize> {
        if self.caret >= self.text.len() {
            return None;
        }
        let mut i = self.caret + 1;
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        Some(i)
    }

    pub fn insert(&mut self, s: &str) {
        if let Some((a, b)) = self.selection() {
            self.text.replace_range(a..b, "");
            self.caret = a;
            self.anchor = None;
        }
        self.text.insert_str(self.caret, s);
        self.caret += s.len();
    }

    fn delete_back(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.text.replace_range(a..b, "");
            self.caret = a;
            self.anchor = None;
        } else if let Some(p) = self.prev_boundary() {
            self.text.replace_range(p..self.caret, "");
            self.caret = p;
        }
    }

    fn delete_forward(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.text.replace_range(a..b, "");
            self.caret = a;
            self.anchor = None;
        } else if let Some(n) = self.next_boundary() {
            self.text.replace_range(self.caret..n, "");
        }
    }
}

/// Outcome of one frame of a text field.
#[derive(Debug, Default, Clone, Copy)]
pub struct TextResponse {
    pub changed: bool,
    pub submitted: bool,
    pub cancelled: bool,
    pub clicked: bool,
}

/// Single-line text field with selection, clipboard and validation-free input.
pub fn text_field(
    ui: &mut Ui,
    id: Id,
    r: Rect,
    state: &mut TextField,
    placeholder: &str,
    monospace_center: bool,
) -> TextResponse {
    let mut resp = TextResponse::default();
    let clicked = ui.interact(id, r, true);
    if clicked {
        ui.state.focus = Some(id);
        resp.clicked = true;
        // Place the caret at the clicked character.
        let rel = ui.input.mouse.x - (r.x + 8.0) - state.scroll_x;
        let mut acc = 0.0;
        let mut caret = state.text.len();
        for (idx, ch) in state.text.char_indices() {
            let w = ui.font.text_width(&ch.to_string(), Metrics::TEXT_SIZE);
            if acc + w * 0.5 > rel {
                caret = idx;
                break;
            }
            acc += w;
        }
        state.caret = caret;
        state.anchor = None;
        if ui.input.double_click {
            state.select_all();
        }
    }
    let focused = ui.state.focus == Some(id);
    if focused {
        // Typed characters and pastes are applied first so that a frame which
        // also carries Enter (fast typing) submits the updated text.
        if let Some(paste) = ui.input.paste.clone() {
            let cleaned: String = paste.chars().filter(|c| !c.is_control()).collect();
            if !cleaned.is_empty() {
                state.insert(&cleaned);
                resp.changed = true;
            }
        }
        let typed: String = ui
            .input
            .text
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if !typed.is_empty() && !ui.input.ctrl && !ui.input.alt {
            state.insert(&typed);
            resp.changed = true;
        }
        for ev in ui.input.keys.clone() {
            match ev.key {
                super::Key::Enter => resp.submitted = true,
                super::Key::Escape => resp.cancelled = true,
                super::Key::Backspace => {
                    state.delete_back();
                    resp.changed = true;
                }
                super::Key::Delete => {
                    state.delete_forward();
                    resp.changed = true;
                }
                super::Key::Left => {
                    if let Some(p) = state.prev_boundary() {
                        state.caret = p;
                    }
                    state.anchor = None;
                }
                super::Key::Right => {
                    if let Some(n) = state.next_boundary() {
                        state.caret = n;
                    }
                    state.anchor = None;
                }
                super::Key::Home => {
                    state.caret = 0;
                    state.anchor = None;
                }
                super::Key::End => {
                    state.caret = state.text.len();
                    state.anchor = None;
                }
                super::Key::Char('a') if ui.input.ctrl => state.select_all(),
                super::Key::Char('c') if ui.input.ctrl => {
                    if let Some((a, b)) = state.selection() {
                        ui.state.clipboard_set = Some(state.text[a..b].to_string());
                    } else if !state.text.is_empty() {
                        ui.state.clipboard_set = Some(state.text.clone());
                    }
                }
                super::Key::Char('x') if ui.input.ctrl => {
                    if let Some((a, b)) = state.selection() {
                        ui.state.clipboard_set = Some(state.text[a..b].to_string());
                        state.text.replace_range(a..b, "");
                        state.caret = a;
                        state.anchor = None;
                        resp.changed = true;
                    }
                }
                _ => {}
            }
        }
    }

    // Painting.
    let theme = ui.theme;
    let border = if focused { theme.accent } else { theme.control_border };
    ui.bmp.fill_round_rect(r, 5.0, theme.control_bg);
    ui.bmp.stroke_round_rect(r, 5.0, border, 1.0);

    let pad = 8.0;
    let inner_h = r.h;
    // Keep the caret visible.
    let caret_x_in_text = ui
        .font
        .text_width(&state.text[..state.caret.min(state.text.len())], Metrics::TEXT_SIZE);
    let text_area_w = r.w - pad * 2.0;
    if caret_x_in_text - state.scroll_x > text_area_w - 2.0 {
        state.scroll_x = caret_x_in_text - text_area_w + 2.0;
    }
    if caret_x_in_text - state.scroll_x < 0.0 {
        state.scroll_x = caret_x_in_text;
    }
    if state.text.is_empty() {
        state.scroll_x = 0.0;
    }

    let baseline = r.y + (inner_h + ui.font.ascent(Metrics::TEXT_SIZE) * 0.72) * 0.5;
    let clip_x0 = r.x + pad;
    // Selection highlight.
    if let Some((a, b)) = state.selection() {
        let x0 = clip_x0 + ui.font.text_width(&state.text[..a], Metrics::TEXT_SIZE) - state.scroll_x;
        let x1 = clip_x0 + ui.font.text_width(&state.text[..b], Metrics::TEXT_SIZE) - state.scroll_x;
        ui.bmp.fill_rect(
            Rect::new(x0.max(clip_x0), r.y + 4.0, (x1 - x0).max(1.0), r.h - 8.0),
            theme.selection,
        );
    }
    // Text (clipped by drawing per-character inside the field).
    let draw_x = clip_x0 - state.scroll_x;
    if state.text.is_empty() && !focused {
        ui.font.draw_text(
            ui.bmp,
            clip_x0,
            baseline,
            Metrics::TEXT_SIZE,
            theme.text_disabled,
            placeholder,
        );
    } else {
        draw_clipped_text(ui, draw_x, baseline, &state.text, r);
    }
    // Caret with a slow blink (1 Hz).
    if focused && ((ui.input.time * 1.6) as i64 % 2 == 0) {
        let cx = (draw_x + caret_x_in_text).round();
        if cx >= r.x + 2.0 && cx <= r.right() - 2.0 {
            ui.bmp.fill_rect(
                Rect::new(cx, r.y + 5.0, 1.5, r.h - 10.0),
                theme.text,
            );
        }
    }
    let _ = (monospace_center, Point::ZERO);
    resp
}

fn draw_clipped_text(ui: &mut Ui, x: f32, baseline: f32, text: &str, clip: Rect) {
    let mut pen = x.round();
    for ch in text.chars() {
        let g = ui.font.glyph(ch, Metrics::TEXT_SIZE, false);
        if g.width > 0 {
            let gx = pen as i32 + g.left;
            let gy = baseline.round() as i32 + g.top;
            if gx as f32 + g.width as f32 >= clip.x + 1.0 && gx as f32 <= clip.right() - 1.0 {
                for row in 0..g.height {
                    for col in 0..g.width {
                        let px = gx + col as i32;
                        let py = gy + row as i32;
                        let cov = g.coverage[(row * g.width + col) as usize];
                        if cov > 0
                            && px as f32 >= clip.x + 1.0
                            && (px as f32) < clip.right() - 1.0
                            && py >= clip.y as i32
                            && (py as f32) < clip.bottom() - 1.0
                        {
                            ui.bmp.blend_coverage(
                                px.max(0) as u32,
                                py.max(0) as u32,
                                ui.theme.text,
                                cov as f32 / 255.0,
                            );
                        }
                    }
                }
            }
        }
        pen += g.advance.round().max(0.0);
    }
}

/// Checkbox-style settings row. Returns true on the frame the box is clicked
/// (the caller owns the value, so a checked box is not reported as a click).
pub fn checkbox(ui: &mut Ui, id: Id, r: Rect, label: &str, value: bool) -> bool {
    let mut clicked = ui.interact(id, r, true);
    if ui.state.focus == Some(id) {
        ui.state.mark_activatable(id);
        clicked |= ui.state.keyboard_activated(id);
    }
    let theme = ui.theme;
    if ui.hovered(r) {
        ui.bmp.fill_round_rect(r.inset_xy(2.0, 1.0), 5.0, theme.control_hover);
    }
    let box_size = 16.0;
    let bx = r.x + 8.0;
    let by = r.y + (r.h - box_size) * 0.5;
    let b = Rect::new(bx, by, box_size, box_size);
    ui.bmp.fill_round_rect(b, 4.0, if value { theme.accent } else { theme.control_bg });
    ui.bmp.stroke_round_rect(
        b,
        4.0,
        if value { theme.accent } else { theme.control_border },
        1.0,
    );
    if value {
        // Check mark.
        let cx = b.center();
        ui.bmp.stroke_line(cx.x - 4.0, cx.y, cx.x - 1.0, cx.y + 3.0, 2.0, theme.accent_text);
        ui.bmp.stroke_line(cx.x - 1.0, cx.y + 3.0, cx.x + 4.0, cx.y - 3.5, 2.0, theme.accent_text);
    }
    if ui.state.focus == Some(id) {
        // Keyboard focus ring, so Tab navigation is visible.
        ui.bmp.stroke_round_rect(
            r.inset(-2.0),
            6.0,
            theme.accent.fade(0.7),
            1.5,
        );
    }
    let baseline = r.y + (r.h + ui.font.ascent(Metrics::TEXT_SIZE) * 0.72) * 0.5;
    let text = ui.font.ellipsize(label, Metrics::TEXT_SIZE, r.right() - b.right() - 20.0);
    ui.font.draw_text(
        ui.bmp,
        b.right() + 10.0,
        baseline,
        Metrics::TEXT_SIZE,
        theme.text,
        &text,
    );
    clicked
}

/// Section heading used inside the sidebar and dialogs.
pub fn section_label(ui: &mut Ui, r: Rect, text: &str) {
    let baseline = r.y + ui.font.ascent(Metrics::SMALL_TEXT_SIZE) + 3.0;
    ui.font.draw_text(
        ui.bmp,
        r.x,
        baseline,
        Metrics::SMALL_TEXT_SIZE,
        ui.theme.text_muted,
        text,
    );
}

/// Clamps a scroll offset for a scrollable region.
pub fn clampi(v: f32, lo: f32, hi: f32) -> f32 {
    clamp(v, lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{canvas::Color, Font, Input, Theme, UiState};

    fn frame<'a>(
        bmp: &'a mut crate::ui::Bitmap,
        font: &'a Font,
        input: &'a Input,
        state: &'a mut UiState,
    ) -> Ui<'a> {
        Ui::new(bmp, font, Theme::light(), input, state)
    }

    #[test]
    fn button_click_requires_press_and_release_inside() {
        let font = Font::ui();
        let mut bmp = crate::ui::Bitmap::filled(200, 60, Color::rgb(255, 255, 255));
        let mut state = UiState::default();
        let r = Rect::new(10.0, 10.0, 80.0, 28.0);

        let mut input = Input {
            mouse: Point::new(20.0, 20.0),
            pressed: true,
            down: true,
            ..Default::default()
        };
        let clicked = {
            let mut ui = frame(&mut bmp, &font, &input, &mut state);
            button(&mut ui, 7, r, "Open", true, false)
        };
        assert!(!clicked);

        input.pressed = false;
        input.released = true;
        input.down = false;
        let clicked = {
            let mut ui = frame(&mut bmp, &font, &input, &mut state);
            button(&mut ui, 7, r, "Open", true, false)
        };
        assert!(clicked);

        // Releasing outside the button must not fire it.
        let mut state = UiState::default();
        let mut input2 = Input {
            mouse: Point::new(150.0, 50.0),
            down: true,
            pressed: true,
            ..Default::default()
        };
        {
            let mut ui = frame(&mut bmp, &font, &input2, &mut state);
            button(&mut ui, 7, r, "Open", true, false);
        }
        input2.released = true;
        input2.down = false;
        let clicked = {
            let mut ui = frame(&mut bmp, &font, &input2, &mut state);
            button(&mut ui, 7, r, "Open", true, false)
        };
        assert!(!clicked);
    }

    #[test]
    fn disabled_button_never_clicks() {
        let font = Font::ui();
        let mut bmp = crate::ui::Bitmap::filled(100, 40, Color::rgb(0, 0, 0));
        let mut state = UiState::default();
        let r = Rect::new(0.0, 0.0, 80.0, 28.0);
        let input = Input {
            mouse: Point::new(10.0, 10.0),
            down: true,
            pressed: true,
            ..Default::default()
        };
        let mut ui = frame(&mut bmp, &font, &input, &mut state);
        assert!(!button(&mut ui, 1, r, "x", false, false));
    }

    #[test]
    fn text_field_edits_and_submits() {
        let font = Font::ui();
        let mut bmp = crate::ui::Bitmap::filled(200, 40, Color::rgb(0, 0, 0));
        let mut state = UiState {
            focus: Some(3),
            ..Default::default()
        };
        let r = Rect::new(0.0, 0.0, 120.0, 28.0);
        let mut field = TextField::with_text("12");
        let mut input = Input {
            text: "3".into(),
            ..Default::default()
        };
        input.keys.push(crate::ui::KeyEvent {
            key: crate::ui::Key::Enter,
            repeat: false,
        });
        let mut ui = frame(&mut bmp, &font, &input, &mut state);
        let resp = text_field(&mut ui, 3, r, &mut field, "page", true);
        assert_eq!(field.text, "123");
        assert!(resp.changed);
        assert!(resp.submitted);

        let mut input = Input::default();
        input.keys.push(crate::ui::KeyEvent {
            key: crate::ui::Key::Backspace,
            repeat: false,
        });
        let mut ui = frame(&mut bmp, &font, &input, &mut state);
        text_field(&mut ui, 3, r, &mut field, "page", true);
        assert_eq!(field.text, "12");
    }

    #[test]
    fn scrollbar_clamps_offset() {
        let font = Font::ui();
        let mut bmp = crate::ui::Bitmap::filled(100, 200, Color::rgb(0, 0, 0));
        let mut state = UiState::default();
        let input = Input::default();
        let mut offset = 5000.0;
        let mut ui = frame(&mut bmp, &font, &input, &mut state);
        scrollbar(&mut ui, 1, Rect::new(90.0, 0.0, 8.0, 200.0), 1000.0, 200.0, &mut offset);
        assert!(offset >= 0.0 && offset <= 800.0);
    }
}
