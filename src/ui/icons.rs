//! Vector icons drawn procedurally on a 16x16 grid.
//!
//! Icons are stroked with anti-aliased lines so no bitmap assets or icon fonts
//! are needed, and they stay crisp at any DPI scale.

use super::canvas::{Bitmap, Color};
use crate::geom::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Menu,
    Open,
    Prev,
    Next,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    RotateCw,
    RotateCcw,
    Search,
    Thumbnails,
    Outline,
    Recent,
    Sun,
    Moon,
    Fullscreen,
    Presentation,
    Print,
    Close,
    ChevronDown,
    Settings,
    Copy,
    Rotate180,
    Info,
    Folder,
    NextMatch,
    PrevMatch,
}

impl Icon {
    /// Human readable label used for tooltips and accessibility.
    pub fn label(self) -> &'static str {
        match self {
            Icon::Menu => "Toggle sidebar",
            Icon::Open => "Open PDF",
            Icon::Prev => "Previous page",
            Icon::Next => "Next page",
            Icon::ZoomIn => "Zoom in",
            Icon::ZoomOut => "Zoom out",
            Icon::FitWidth => "Fit to width",
            Icon::FitPage => "Fit page",
            Icon::RotateCw => "Rotate clockwise",
            Icon::RotateCcw => "Rotate counter-clockwise",
            Icon::Rotate180 => "Rotate 180°",
            Icon::Search => "Search",
            Icon::Thumbnails => "Thumbnails",
            Icon::Outline => "Contents",
            Icon::Recent => "Recent documents",
            Icon::Sun => "Light theme",
            Icon::Moon => "Dark theme",
            Icon::Fullscreen => "Full screen",
            Icon::Presentation => "Presentation mode",
            Icon::Print => "Print",
            Icon::Close => "Close",
            Icon::ChevronDown => "More",
            Icon::Settings => "Settings",
            Icon::Copy => "Copy",
            Icon::Info => "Document info",
            Icon::Folder => "Folder",
            Icon::NextMatch => "Next match",
            Icon::PrevMatch => "Previous match",
        }
    }
}

/// Draws `icon` centred in `area`.
pub fn draw(bmp: &mut Bitmap, area: Rect, icon: Icon, color: Color) {
    let size = area.w.min(area.h);
    let scale = size / 16.0;
    let ox = area.x + (area.w - size) * 0.5;
    let oy = area.y + (area.h - size) * 0.5;
    let w = (1.45 * scale).max(1.0);
    let p = |x: f32, y: f32| (ox + x * scale, oy + y * scale);
    let line = |bmp: &mut Bitmap, x0: f32, y0: f32, x1: f32, y1: f32| {
        let (a, b) = p(x0, y0);
        let (c, d) = p(x1, y1);
        bmp.stroke_line(a, b, c, d, w, color);
    };
    let stroke_rect = |bmp: &mut Bitmap, x: f32, y: f32, rw: f32, rh: f32, width: f32| {
        let (a, b) = p(x, y);
        bmp.stroke_round_rect(Rect::new(a, b, rw * scale, rh * scale), 1.5 * scale, color, width * scale);
    };
    let circle = |bmp: &mut Bitmap, cx: f32, cy: f32, r: f32| {
        let (a, b) = p(cx, cy);
        bmp.fill_circle(a, b, r * scale, color);
    };
    let ring = |bmp: &mut Bitmap, cx: f32, cy: f32, r: f32| {
        let steps = 32;
        for i in 0..steps {
            let a0 = i as f32 / steps as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / steps as f32 * std::f32::consts::TAU;
            line(
                bmp,
                cx + r * a0.cos(),
                cy + r * a0.sin(),
                cx + r * a1.cos(),
                cy + r * a1.sin(),
            );
        }
    };
    match icon {
        Icon::Menu => {
            for y in [4.0f32, 8.0, 12.0] {
                line(bmp, 3.0, y, 13.0, y);
            }
        }
        Icon::Open => {
            line(bmp, 2.0, 12.5, 2.0, 5.0);
            line(bmp, 2.0, 5.0, 6.5, 5.0);
            line(bmp, 6.5, 5.0, 8.0, 7.0);
            line(bmp, 8.0, 7.0, 13.5, 7.0);
            line(bmp, 13.5, 7.0, 13.5, 12.5);
            line(bmp, 2.0, 12.5, 13.5, 12.5);
        }
        Icon::Folder => {
            line(bmp, 2.0, 12.5, 2.0, 4.5);
            line(bmp, 2.0, 4.5, 6.0, 4.5);
            line(bmp, 6.0, 4.5, 7.5, 6.5);
            line(bmp, 7.5, 6.5, 14.0, 6.5);
            line(bmp, 14.0, 6.5, 14.0, 12.5);
            line(bmp, 2.0, 12.5, 14.0, 12.5);
        }
        Icon::Prev | Icon::Next => {
            let dir = if icon == Icon::Prev { -1.0 } else { 1.0 };
            line(bmp, 8.0 - 2.5 * dir, 4.0, 8.0 + 2.5 * dir, 8.0);
            line(bmp, 8.0 + 2.5 * dir, 8.0, 8.0 - 2.5 * dir, 12.0);
        }
        Icon::PrevMatch | Icon::NextMatch => {
            let dir = if icon == Icon::PrevMatch { -1.0 } else { 1.0 };
            line(bmp, 8.0 - 2.5 * dir, 4.0, 8.0 + 2.5 * dir, 8.0);
            line(bmp, 8.0 + 2.5 * dir, 8.0, 8.0 - 2.5 * dir, 12.0);
            line(bmp, 3.0, 13.8, 13.0, 13.8);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            ring(bmp, 7.0, 7.0, 4.6);
            line(bmp, 10.6, 10.6, 14.0, 14.0);
            line(bmp, 4.8, 7.0, 9.2, 7.0);
            if icon == Icon::ZoomIn {
                line(bmp, 7.0, 4.8, 7.0, 9.2);
            }
        }
        Icon::FitWidth => {
            stroke_rect(bmp, 2.0, 3.5, 12.0, 9.0, 1.2);
            line(bmp, 4.0, 8.0, 7.5, 8.0);
            line(bmp, 8.5, 8.0, 12.0, 8.0);
        }
        Icon::FitPage => {
            stroke_rect(bmp, 3.5, 2.0, 9.0, 12.0, 1.2);
            stroke_rect(bmp, 5.5, 4.0, 5.0, 8.0, 0.9);
        }
        Icon::RotateCw | Icon::RotateCcw => {
            let steps = 24;
            let start = if icon == Icon::RotateCw { -1.9 } else { 1.6 };
            let span = 4.6;
            for i in 0..steps {
                let a0 = start + span * i as f32 / steps as f32;
                let a1 = start + span * (i + 1) as f32 / steps as f32;
                line(bmp, 8.0 + 5.0 * a0.cos(), 8.0 + 5.0 * a0.sin(), 8.0 + 5.0 * a1.cos(), 8.0 + 5.0 * a1.sin());
            }
            let tip = start + span;
            let (tx, ty) = (8.0 + 5.0 * tip.cos(), 8.0 + 5.0 * tip.sin());
            line(bmp, tx, ty, tx - 2.4, ty - 0.6);
            line(bmp, tx, ty, tx + 0.2, ty + 2.4);
        }
        Icon::Rotate180 => {
            ring(bmp, 8.0, 8.0, 4.6);
            line(bmp, 8.0, 3.4, 6.0, 5.0);
            line(bmp, 8.0, 3.4, 10.0, 5.0);
            line(bmp, 8.0, 12.6, 6.0, 11.0);
            line(bmp, 8.0, 12.6, 10.0, 11.0);
        }
        Icon::Search => {
            ring(bmp, 7.0, 7.0, 4.3);
            line(bmp, 10.4, 10.4, 13.8, 13.8);
        }
        Icon::Thumbnails => {
            for i in 0..3 {
                let x = 2.5 + i as f32 * 4.0;
                stroke_rect(bmp, x, 3.5, 3.0, 4.0, 1.0);
                stroke_rect(bmp, x, 9.0, 3.0, 4.0, 1.0);
            }
        }
        Icon::Outline => {
            for i in 0..4 {
                let y = 3.5 + i as f32 * 3.0;
                circle(bmp, 3.0, y, 0.9);
                line(bmp, 5.5, y, 13.5, y);
            }
        }
        Icon::Recent => {
            ring(bmp, 8.0, 8.0, 5.2);
            line(bmp, 8.0, 4.6, 8.0, 8.2);
            line(bmp, 8.0, 8.2, 10.8, 9.6);
        }
        Icon::Sun => {
            circle(bmp, 8.0, 8.0, 3.2);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                line(bmp, 8.0 + 4.6 * a.cos(), 8.0 + 4.6 * a.sin(), 8.0 + 6.4 * a.cos(), 8.0 + 6.4 * a.sin());
            }
        }
        Icon::Moon => {
            let steps = 28;
            for i in 0..steps {
                let a0 = -1.2 + 3.9 * i as f32 / steps as f32;
                let a1 = -1.2 + 3.9 * (i + 1) as f32 / steps as f32;
                line(bmp, 9.0 + 5.0 * a0.cos(), 8.0 + 5.0 * a0.sin(), 9.0 + 5.0 * a1.cos(), 8.0 + 5.0 * a1.sin());
            }
            let steps2 = 24;
            for i in 0..steps2 {
                let a0 = -1.2 + 3.9 * i as f32 / steps2 as f32;
                let a1 = -1.2 + 3.9 * (i + 1) as f32 / steps2 as f32;
                line(bmp, 4.2 + 3.4 * a0.cos(), 8.2 + 3.4 * a0.sin(), 4.2 + 3.4 * a1.cos(), 8.2 + 3.4 * a1.sin());
            }
        }
        Icon::Fullscreen => {
            line(bmp, 3.0, 6.5, 3.0, 3.0);
            line(bmp, 3.0, 3.0, 6.5, 3.0);
            line(bmp, 13.0, 6.5, 13.0, 3.0);
            line(bmp, 13.0, 3.0, 9.5, 3.0);
            line(bmp, 3.0, 9.5, 3.0, 13.0);
            line(bmp, 3.0, 13.0, 6.5, 13.0);
            line(bmp, 13.0, 9.5, 13.0, 13.0);
            line(bmp, 13.0, 13.0, 9.5, 13.0);
        }
        Icon::Presentation => {
            stroke_rect(bmp, 2.0, 2.5, 12.0, 8.5, 1.2);
            line(bmp, 8.0, 11.0, 8.0, 13.5);
            line(bmp, 5.5, 13.5, 10.5, 13.5);
        }
        Icon::Print => {
            stroke_rect(bmp, 4.0, 2.5, 8.0, 3.5, 1.0);
            stroke_rect(bmp, 2.0, 6.0, 12.0, 6.0, 1.2);
            stroke_rect(bmp, 4.5, 10.0, 7.0, 3.5, 1.0);
        }
        Icon::Close => {
            line(bmp, 4.0, 4.0, 12.0, 12.0);
            line(bmp, 12.0, 4.0, 4.0, 12.0);
        }
        Icon::ChevronDown => {
            line(bmp, 4.5, 6.5, 8.0, 10.0);
            line(bmp, 8.0, 10.0, 11.5, 6.5);
        }
        Icon::Settings => {
            ring(bmp, 8.0, 8.0, 5.0);
            circle(bmp, 8.0, 8.0, 1.8);
            for i in 0..4 {
                let a = i as f32 / 4.0 * std::f32::consts::TAU + 0.4;
                line(bmp, 8.0 + 5.0 * a.cos(), 8.0 + 5.0 * a.sin(), 8.0 + 6.8 * a.cos(), 8.0 + 6.8 * a.sin());
            }
        }
        Icon::Copy => {
            stroke_rect(bmp, 2.5, 2.5, 8.0, 9.0, 1.1);
            stroke_rect(bmp, 5.5, 5.5, 8.0, 9.0, 1.1);
        }
        Icon::Info => {
            ring(bmp, 8.0, 8.0, 5.4);
            circle(bmp, 8.0, 4.8, 0.9);
            line(bmp, 8.0, 7.0, 8.0, 11.5);
        }
    }
}

/// Renders `icon` into a standalone square bitmap (used for thumbnails/tests).
pub fn render(icon: Icon, size: u32, color: Color) -> Bitmap {
    let mut bmp = Bitmap::filled(size, size, Color::TRANSPARENT);
    draw(&mut bmp, Rect::new(0.0, 0.0, size as f32, size as f32), icon, color);
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws_something() {
        for icon in [
            Icon::Menu,
            Icon::Open,
            Icon::Prev,
            Icon::Next,
            Icon::ZoomIn,
            Icon::ZoomOut,
            Icon::FitWidth,
            Icon::FitPage,
            Icon::RotateCw,
            Icon::RotateCcw,
            Icon::Search,
            Icon::Thumbnails,
            Icon::Outline,
            Icon::Recent,
            Icon::Sun,
            Icon::Moon,
            Icon::Fullscreen,
            Icon::Presentation,
            Icon::Print,
            Icon::Close,
            Icon::ChevronDown,
            Icon::Settings,
            Icon::Copy,
            Icon::Rotate180,
            Icon::Info,
            Icon::Folder,
            Icon::NextMatch,
            Icon::PrevMatch,
        ] {
            let bmp = render(icon, 20, Color::rgb(0, 0, 0));
            let inked = bmp.px.iter().filter(|p| (**p >> 24) > 0).count();
            assert!(inked > 8, "{:?} drew only {} pixels", icon, inked);
        }
    }

    #[test]
    fn icons_are_labelled() {
        assert!(!Icon::Open.label().is_empty());
    }
}
