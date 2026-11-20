//! Colour palettes and layout metrics for the light and dark themes.

use super::canvas::Color;

/// Every colour the UI needs. Kept flat and `Copy` so themes are cheap to swap.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub window_bg: Color,
    pub toolbar_bg: Color,
    pub toolbar_border: Color,
    pub sidebar_bg: Color,
    pub status_bg: Color,
    pub viewer_bg: Color,
    pub control_bg: Color,
    pub control_hover: Color,
    pub control_active: Color,
    pub control_border: Color,
    pub control_disabled: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub accent_soft: Color,
    pub divider: Color,
    pub scrollbar: Color,
    pub scrollbar_hover: Color,
    pub page_border: Color,
    pub shadow: Color,
    pub selection: Color,
    pub search_hit: Color,
    pub search_hit_active: Color,
    pub link: Color,
    pub error: Color,
    pub warning: Color,
    pub toast_bg: Color,
    pub toast_text: Color,
    pub tooltip_bg: Color,
    pub tooltip_text: Color,
    pub modal_scrim: Color,
    pub modal_bg: Color,
}

impl Theme {
    pub fn light() -> Theme {
        Theme {
            dark: false,
            window_bg: Color::hex(0xFFFFFF),
            toolbar_bg: Color::hex(0xF6F7F9),
            toolbar_border: Color::hex(0xD9DDE3),
            sidebar_bg: Color::hex(0xFAFBFC),
            status_bg: Color::hex(0xF6F7F9),
            viewer_bg: Color::hex(0xE8EAEE),
            control_bg: Color::hex(0xFFFFFF),
            control_hover: Color::hex(0xEDF0F4),
            control_active: Color::hex(0xE1E6EC),
            control_border: Color::hex(0xD3D8DF),
            control_disabled: Color::hex(0xF1F3F5),
            text: Color::hex(0x1B1D21),
            text_muted: Color::hex(0x606773),
            text_disabled: Color::hex(0xA3A9B3),
            accent: Color::hex(0x2F6FEB),
            accent_text: Color::hex(0xFFFFFF),
            accent_soft: Color::rgba(0x2F, 0x6F, 0xEB, 0x2A),
            divider: Color::hex(0xE2E5EA),
            scrollbar: Color::rgba(0x40, 0x46, 0x50, 0x55),
            scrollbar_hover: Color::rgba(0x2C, 0x31, 0x38, 0x8C),
            page_border: Color::hex(0xCFD4DA),
            shadow: Color::rgba(0x20, 0x28, 0x33, 0x40),
            selection: Color::rgba(0x2F, 0x6F, 0xEB, 0x50),
            search_hit: Color::rgba(0xFF, 0xC8, 0x33, 0x66),
            search_hit_active: Color::rgba(0xFF, 0xA0, 0x00, 0x99),
            link: Color::hex(0x1C5FD8),
            error: Color::hex(0xC0392B),
            warning: Color::hex(0xB9770E),
            toast_bg: Color::rgba(0x21, 0x25, 0x2B, 0xF2),
            toast_text: Color::hex(0xFFFFFF),
            tooltip_bg: Color::rgba(0x21, 0x25, 0x2B, 0xF2),
            tooltip_text: Color::hex(0xF5F6F8),
            modal_scrim: Color::rgba(0x15, 0x18, 0x1C, 0x66),
            modal_bg: Color::hex(0xFFFFFF),
        }
    }

    pub fn dark() -> Theme {
        Theme {
            dark: true,
            window_bg: Color::hex(0x17191C),
            toolbar_bg: Color::hex(0x1E2125),
            toolbar_border: Color::hex(0x2C3137),
            sidebar_bg: Color::hex(0x1A1D20),
            status_bg: Color::hex(0x1E2125),
            viewer_bg: Color::hex(0x101214),
            control_bg: Color::hex(0x24282D),
            control_hover: Color::hex(0x2E343A),
            control_active: Color::hex(0x383F47),
            control_border: Color::hex(0x343A41),
            control_disabled: Color::hex(0x1C1F23),
            text: Color::hex(0xE7EAEE),
            text_muted: Color::hex(0x9AA2AC),
            text_disabled: Color::hex(0x5A626C),
            accent: Color::hex(0x4C8DFF),
            accent_text: Color::hex(0x0B1220),
            accent_soft: Color::rgba(0x4C, 0x8D, 0xFF, 0x38),
            divider: Color::hex(0x2A2F35),
            scrollbar: Color::rgba(0xC8, 0xD0, 0xDA, 0x33),
            scrollbar_hover: Color::rgba(0xC8, 0xD0, 0xDA, 0x66),
            page_border: Color::hex(0x2B3036),
            shadow: Color::rgba(0x00, 0x00, 0x00, 0x80),
            selection: Color::rgba(0x4C, 0x8D, 0xFF, 0x55),
            search_hit: Color::rgba(0xFF, 0xC8, 0x33, 0x55),
            search_hit_active: Color::rgba(0xFF, 0xA5, 0x1A, 0x8C),
            link: Color::hex(0x7FB0FF),
            error: Color::hex(0xFF7B72),
            warning: Color::hex(0xF2C04C),
            toast_bg: Color::rgba(0x2B, 0x31, 0x38, 0xF5),
            toast_text: Color::hex(0xF2F4F7),
            tooltip_bg: Color::rgba(0x36, 0x3D, 0x45, 0xF7),
            tooltip_text: Color::hex(0xF2F4F7),
            modal_scrim: Color::rgba(0x00, 0x00, 0x00, 0x99),
            modal_bg: Color::hex(0x1F2327),
        }
    }

    pub fn from_dark(dark: bool) -> Theme {
        if dark {
            Theme::dark()
        } else {
            Theme::light()
        }
    }
}

/// Layout metrics shared by all viewport sizes (logical pixels).
pub struct Metrics;

impl Metrics {
    pub const TOOLBAR_H: f32 = 46.0;
    pub const TABBAR_H: f32 = 30.0;
    pub const STATUS_H: f32 = 24.0;
    pub const SIDEBAR_DEFAULT_W: f32 = 220.0;
    pub const SIDEBAR_MIN_W: f32 = 130.0;
    pub const SIDEBAR_MAX_W: f32 = 520.0;
    pub const CONTROL_H: f32 = 28.0;
    pub const BUTTON_RADIUS: f32 = 6.0;
    pub const TEXT_SIZE: f32 = 13.0;
    pub const SMALL_TEXT_SIZE: f32 = 11.0;
    pub const TITLE_TEXT_SIZE: f32 = 14.0;
    pub const PAGE_GAP: f32 = 14.0;
    pub const SCROLLBAR_W: f32 = 10.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_differ_in_luminance() {
        let l = Theme::light();
        let d = Theme::dark();
        assert!(l.viewer_bg.r() > d.viewer_bg.r());
        assert!(!l.dark && d.dark);
    }

    #[test]
    fn both_themes_are_opaque_where_needed() {
        for t in [Theme::light(), Theme::dark()] {
            assert_eq!(t.window_bg.a(), 255);
            assert_eq!(t.text.a(), 255);
            // Selection overlays must be translucent to reveal page content.
            assert!(t.selection.a() < 255);
        }
    }
}
