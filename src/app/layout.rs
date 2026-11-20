//! View layout mathematics: zoom resolution, page geometry, scroll anchoring
//! and hit testing. Pure functions — unit tested without any GUI.

use crate::db::{ReadingMode, SessionRow, ZoomMode};
use crate::geom::{Point, Rect, Size};
use crate::pdf::DocInfo;

/// Page margins inside the viewport (logical pixels).
pub const VIEW_MARGIN: f32 = 16.0;
/// Vertical gap between pages in continuous mode.
pub const PAGE_GAP: f32 = 14.0;
/// Extra pages rendered above and below the viewport.
pub const PREFETCH_PAGES: usize = 1;
/// Zoom limits.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 8.0;

/// Placement of one page in document space (document origin = top of page 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeom {
    pub page: u32,
    /// Offset of the page's top-left corner inside the document.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PageGeom {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

/// Result of laying out a document for a given viewport.
#[derive(Debug, Clone, Default)]
pub struct ViewLayout {
    /// Effective zoom: device pixels per PDF point.
    pub zoom: f32,
    pub pages: Vec<PageGeom>,
    /// Total height of the (continuous) document in pixels.
    pub total_h: f32,
    pub viewport: Size,
    /// Scroll offset applied when the layout was built (clamped).
    pub scroll: f32,
    /// True when the document is narrower than the viewport (pages centred).
    pub single_page: bool,
}

impl ViewLayout {
    /// Maximum scroll offset for this layout.
    pub fn max_scroll(&self) -> f32 {
        (self.total_h - self.viewport.h).max(0.0)
    }

    /// Screen rectangle of a page for the current scroll offset.
    pub fn page_screen_rect(&self, geom: &PageGeom, viewport: Rect) -> Rect {
        Rect::new(
            viewport.x + geom.x,
            viewport.y + geom.y - self.scroll,
            geom.w,
            geom.h,
        )
    }

    /// The page that occupies the centre of the viewport.
    pub fn current_page(&self) -> u32 {
        if self.pages.is_empty() {
            return 0;
        }
        let centre = self.scroll + self.viewport.h * 0.5;
        let mut best = self.pages[0].page;
        let mut best_dist = f32::MAX;
        for p in &self.pages {
            let pc = p.y + p.h * 0.5;
            let dist = (pc - centre).abs();
            if dist < best_dist {
                best_dist = dist;
                best = p.page;
            }
        }
        best
    }

    /// Pages that intersect the viewport, including `PREFETCH_PAGES` on each
    /// side. Ordered top to bottom.
    pub fn visible_pages(&self) -> Vec<u32> {
        let top = self.scroll - self.viewport.h * PREFETCH_PAGES as f32;
        let bottom = self.scroll + self.viewport.h * (PREFETCH_PAGES as f32 + 1.0);
        self.pages
            .iter()
            .filter(|p| p.y + p.h >= top && p.y <= bottom)
            .map(|p| p.page)
            .collect()
    }

    /// Offset of a page's top edge, in document space.
    pub fn page_offset(&self, page: u32) -> f32 {
        self.pages
            .iter()
            .find(|p| p.page == page)
            .map(|p| p.y)
            .unwrap_or(0.0)
    }

    /// The fraction of `page` that is scrolled past the viewport top (0..1).
    pub fn scroll_fraction(&self, page: u32) -> f32 {
        let Some(geom) = self.pages.iter().find(|p| p.page == page) else {
            return 0.0;
        };
        let inside = (self.scroll - geom.y) / geom.h.max(1.0);
        if self.pages.len() <= 1 {
            // Single-page mode: the fraction is measured against the scrollable
            // range so restoring works even when the page is taller than the view.
            let scrollable = (geom.h - self.viewport.h).max(1.0);
            (self.scroll / scrollable).clamp(0.0, 1.0)
        } else {
            inside.clamp(0.0, 1.0)
        }
    }

    /// Scroll offset that brings `page`'s `fraction` to the viewport top.
    pub fn scroll_for(&self, page: u32, fraction: f32) -> f32 {
        let Some(geom) = self.pages.iter().find(|p| p.page == page) else {
            return 0.0;
        };
        let target = if self.pages.len() <= 1 {
            let scrollable = (geom.h - self.viewport.h).max(0.0);
            scrollable * fraction.clamp(0.0, 1.0)
        } else {
            geom.y + geom.h * fraction.clamp(0.0, 1.0)
        };
        target.clamp(0.0, self.max_scroll())
    }

    /// Page under a viewport-relative point, with the point in page space.
    pub fn hit_test(&self, viewport: Rect, point: Point) -> Option<(u32, Point)> {
        for geom in &self.pages {
            let r = self.page_screen_rect(geom, viewport);
            if r.contains(point) {
                return Some((
                    geom.page,
                    Point::new((point.x - r.x) / self.zoom, (point.y - r.y) / self.zoom),
                ));
            }
        }
        None
    }
}

/// Resolves the effective zoom for a session.
pub fn resolve_zoom(
    mode: ZoomMode,
    custom_zoom: f32,
    info: &DocInfo,
    page: u32,
    rotation: u16,
    viewport: Size,
) -> f32 {
    let available_w = (viewport.w - VIEW_MARGIN * 2.0).max(64.0);
    let available_h = (viewport.h - VIEW_MARGIN * 2.0).max(64.0);
    let size = info.display_size(page, rotation);
    let zoom = match mode {
        ZoomMode::FitWidth => available_w / size.w.max(1.0),
        ZoomMode::FitPage => (available_w / size.w.max(1.0)).min(available_h / size.h.max(1.0)),
        ZoomMode::Actual => 1.0,
        ZoomMode::Custom => custom_zoom,
    };
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// Builds the page layout for a document.
pub fn build_layout(
    info: &DocInfo,
    session: &SessionRow,
    viewport: Size,
    scroll: f32,
) -> ViewLayout {
    let page_count = info.page_count.max(1);
    let current = session.last_page.clamp(1, page_count);
    let zoom = resolve_zoom(
        session.zoom_mode,
        session.zoom,
        info,
        current - 1,
        session.rotation,
        viewport,
    );
    let single = session.reading_mode == ReadingMode::Single;

    let pages_range: Vec<u32> = if single {
        vec![current - 1]
    } else {
        (0..page_count).collect()
    };

    let mut pages = Vec::with_capacity(pages_range.len());
    let mut y = VIEW_MARGIN;
    for page in pages_range {
        let size = info.display_size(page, session.rotation);
        let w = (size.w * zoom).round().max(1.0);
        let h = (size.h * zoom).round().max(1.0);
        let x = ((viewport.w - w) * 0.5).max(0.0).round();
        pages.push(PageGeom { page, x, y, w, h });
        y += h + PAGE_GAP;
    }
    let total_h = if pages.is_empty() {
        0.0
    } else {
        y - PAGE_GAP + VIEW_MARGIN
    };
    let max_scroll = (total_h - viewport.h).max(0.0);
    let mut layout = ViewLayout {
        zoom,
        pages,
        total_h,
        viewport,
        scroll: scroll.clamp(0.0, max_scroll),
        single_page: single,
    };
    // In single-page mode the scroll is relative to the page, not the document.
    if single {
        layout.scroll = scroll.clamp(0.0, (layout.total_h - viewport.h).max(0.0));
    }
    layout
}

/// Zoom levels used by Ctrl+wheel and the zoom buttons.
/// Height of the sidebar's tab strip.
pub const SIDEBAR_TABS_H: f32 = 34.0;

/// Tab strip of the sidebar.
pub fn sidebar_tabs(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.w - 1.0, SIDEBAR_TABS_H)
}

/// Scrollable body below the sidebar's tab strip.
pub fn sidebar_body(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y + SIDEBAR_TABS_H + 1.0,
        area.w - 1.0,
        (area.h - SIDEBAR_TABS_H - 1.0).max(0.0),
    )
}

/// Where the page thumbnails are drawn in the sidebar.
///
/// Shared by the painter and by [`crate::app::App::thumbnail_cells`] so hit
/// testing, scrolling and drawing can never disagree.
pub struct ThumbCells {
    pub cells: Vec<Rect>,
    pub content_h: f32,
}

pub fn thumbnail_cells(body: Rect, sizes: &[Size], scroll: f32) -> ThumbCells {
    let pad = 10.0;
    let cell_w = (body.w - pad * 2.0 - crate::ui::Metrics::SCROLLBAR_W).max(60.0);
    let mut y = body.y + 8.0 - scroll;
    let mut cells = Vec::with_capacity(sizes.len());
    let mut content_h = 12.0;
    for size in sizes {
        let thumb_h = (cell_w * size.h / size.w.max(1.0)).clamp(40.0, 400.0);
        cells.push(Rect::new(body.x + pad, y, cell_w, thumb_h));
        content_h += thumb_h + 24.0;
        y += thumb_h + 24.0;
    }
    ThumbCells { cells, content_h }
}

pub const ZOOM_STEPS: &[f32] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 6.0, 8.0,
];

/// Next zoom step above `current`.
pub fn zoom_in(current: f32) -> f32 {
    ZOOM_STEPS
        .iter()
        .copied()
        .find(|z| *z > current + 0.001)
        .unwrap_or(MAX_ZOOM)
        .clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Next zoom step below `current`.
pub fn zoom_out(current: f32) -> f32 {
    ZOOM_STEPS
        .iter()
        .copied()
        .rev()
        .find(|z| *z < current - 0.001)
        .unwrap_or(MIN_ZOOM)
        .clamp(MIN_ZOOM, MAX_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Size;

    fn info(pages: u32, w: f32, h: f32) -> DocInfo {
        DocInfo {
            page_count: pages,
            page_sizes: vec![Size::new(w, h); pages as usize],
            page_rotations: vec![0; pages as usize],
            ..Default::default()
        }
    }

    #[test]
    fn fit_page_matches_the_smaller_ratio() {
        let info = info(1, 600.0, 800.0);
        let viewport = Size::new(1000.0, 600.0);
        let zoom = resolve_zoom(ZoomMode::FitPage, 1.0, &info, 0, 0, viewport);
        // Vertical is the constraint: (600 - 32) / 800.
        assert!((zoom - (568.0 / 800.0)).abs() < 1e-4, "zoom={zoom}");
    }

    #[test]
    fn fit_width_uses_the_width_ratio() {
        let info = info(1, 600.0, 800.0);
        let viewport = Size::new(1000.0, 600.0);
        let zoom = resolve_zoom(ZoomMode::FitWidth, 1.0, &info, 0, 0, viewport);
        assert!((zoom - (968.0 / 600.0)).abs() < 1e-4);
    }

    #[test]
    fn rotation_affects_fit_zoom() {
        let info = info(1, 600.0, 800.0);
        let viewport = Size::new(1000.0, 600.0);
        let portrait = resolve_zoom(ZoomMode::FitPage, 1.0, &info, 0, 0, viewport);
        let landscape = resolve_zoom(ZoomMode::FitPage, 1.0, &info, 0, 90, viewport);
        assert!(landscape > portrait);
    }

    #[test]
    fn continuous_layout_stacks_pages_with_gaps() {
        let info = info(3, 100.0, 200.0);
        let session = SessionRow {
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let layout = build_layout(&info, &session, Size::new(400.0, 300.0), 0.0);
        assert_eq!(layout.pages.len(), 3);
        assert_eq!(layout.zoom, 1.0);
        assert!((layout.pages[0].y - VIEW_MARGIN).abs() < 0.01);
        assert!((layout.pages[1].y - (VIEW_MARGIN + 200.0 + PAGE_GAP)).abs() < 0.01);
        assert!((layout.total_h - (VIEW_MARGIN * 2.0 + 600.0 + PAGE_GAP * 2.0)).abs() < 0.01);
        // Pages are centred horizontally.
        assert!((layout.pages[0].x - 150.0).abs() < 1.0);
    }

    #[test]
    fn single_page_layout_only_contains_the_current_page() {
        let info = info(10, 100.0, 200.0);
        let session = SessionRow {
            reading_mode: ReadingMode::Single,
            last_page: 4,
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let layout = build_layout(&info, &session, Size::new(400.0, 300.0), 0.0);
        assert_eq!(layout.pages.len(), 1);
        assert_eq!(layout.pages[0].page, 3);
        assert!(layout.single_page);
    }

    #[test]
    fn current_page_follows_the_viewport_centre() {
        let info = info(5, 100.0, 200.0);
        let session = SessionRow {
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let viewport = Size::new(200.0, 200.0);
        let layout = build_layout(&info, &session, viewport, 0.0);
        assert_eq!(layout.current_page(), 0);
        let per_page = 200.0 + PAGE_GAP;
        let layout = build_layout(&info, &session, viewport, per_page * 2.0);
        assert_eq!(layout.current_page(), 2);
    }

    #[test]
    fn visible_pages_include_prefetch() {
        let info = info(20, 100.0, 200.0);
        let session = SessionRow {
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let layout = build_layout(&info, &session, Size::new(200.0, 200.0), 0.0);
        let visible = layout.visible_pages();
        assert!(visible.contains(&0));
        assert!(visible.contains(&1));
        assert!(!visible.contains(&10));
    }

    #[test]
    fn scroll_fraction_round_trips() {
        let info = info(4, 100.0, 400.0);
        let session = SessionRow {
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let viewport = Size::new(300.0, 300.0);
        let layout = build_layout(&info, &session, viewport, 0.0);
        let page = 2;
        let scroll = layout.page_offset(page) + layout.pages[2].h * 0.4;
        let layout = build_layout(&info, &session, viewport, scroll);
        let frac = layout.scroll_fraction(page);
        assert!((frac - 0.4).abs() < 0.01, "fraction={frac}");
        let restored = layout.scroll_for(page, frac);
        assert!((restored - scroll).abs() < 1.0);
    }

    #[test]
    fn hit_testing_maps_into_page_space() {
        let info = info(2, 100.0, 100.0);
        let session = SessionRow {
            zoom_mode: ZoomMode::Actual,
            ..Default::default()
        };
        let viewport = Rect::new(0.0, 0.0, 300.0, 400.0);
        let layout = build_layout(&info, &session, viewport.size(), 0.0);
        let geom = layout.pages[0];
        let inside = Point::new(viewport.x + geom.x + 10.0, viewport.y + geom.y + 20.0);
        let (page, local) = layout.hit_test(viewport, inside).unwrap();
        assert_eq!(page, 0);
        assert!((local.x - 10.0).abs() < 0.01);
        assert!((local.y - 20.0).abs() < 0.01);
        assert!(layout.hit_test(viewport, Point::new(5.0, 5.0)).is_none());
    }

    #[test]
    fn zoom_steps_clamp_and_round_trip() {
        assert!((zoom_in(1.0) - 1.1).abs() < 1e-6);
        assert!((zoom_out(1.0) - 0.9).abs() < 1e-6);
        assert_eq!(zoom_in(MAX_ZOOM), MAX_ZOOM);
        assert_eq!(zoom_out(MIN_ZOOM), MIN_ZOOM);
    }

    #[test]
    fn default_session_zoom_is_sanitised() {
        let info = info(1, 100.0, 100.0);
        let mut session = SessionRow {
            zoom_mode: ZoomMode::Custom,
            zoom: f32::NAN,
            ..Default::default()
        };
        session.clamp_values();
        let layout = build_layout(&info, &session, Size::new(400.0, 400.0), 0.0);
        assert!(layout.zoom.is_finite());
    }
}
