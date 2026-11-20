//! Per-document view state: layout, scroll, zoom, rotation, selection,
//! highlights, search and the bounded bitmap cache.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::app::cache::PageCache;
use crate::app::layout::{self, ViewLayout};
use crate::db::{HighlightRow, ReadingMode, SessionRow, ZoomMode};
use crate::doc::identity::DocIdentity;
use crate::geom::{Point, Rect, Size};
use crate::pdf::{DocId, DocInfo, LinkRect, LinkTarget, OutlineItem, PageText, RenderKey, SearchHit};
use crate::ui::Bitmap;

/// Whether rotation applies to the current page or to the whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationScope {
    Page,
    All,
}

#[derive(Debug, Clone)]
pub enum DocState {
    Loading,
    Ready(Arc<DocInfo>),
    /// The document needs a password before it can be opened.
    NeedsPassword,
    Failed(String),
}

/// Text selection inside one page (character index range).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub page: u32,
    pub anchor: usize,
    pub focus: usize,
    /// True while the user is still dragging.
    pub dragging: bool,
}

impl Selection {
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

/// Full-text search state for one document.
#[derive(Debug, Default, Clone)]
pub struct SearchState {
    pub query: String,
    pub match_case: bool,
    pub hits: Vec<SearchHit>,
    pub current: usize,
    /// Page currently being searched by the worker.
    pub scanning_page: Option<u32>,
    pub active: bool,
    pub no_more_pages: bool,
    /// Monotonic id so stale worker replies can be ignored.
    pub token: u64,
}

impl SearchState {
    pub fn current_hit(&self) -> Option<SearchHit> {
        self.hits.get(self.current).copied()
    }
}

/// One open document.
pub struct DocView {
    pub id: DocId,
    pub identity: DocIdentity,
    pub row_id: Option<i64>,
    pub title: String,
    pub state: DocState,
    pub session: SessionRow,
    /// Scroll offset in pixels (document space, or page space in single mode).
    pub scroll: f32,
    pub layout: ViewLayout,
    pub cache: PageCache,
    pub pending_renders: HashSet<RenderKey>,
    pub texts: HashMap<u32, Arc<PageText>>,
    pub pending_text: HashSet<u32>,
    pub selection: Option<Selection>,
    pub highlights: Vec<HighlightRow>,
    pub outline: Vec<OutlineItem>,
    pub outline_requested: bool,
    pub expanded: HashSet<String>,
    pub links: HashMap<u32, Vec<LinkRect>>,
    pub pending_links: HashSet<u32>,
    pub search: SearchState,
    pub thumbs: HashMap<u32, Option<Arc<Bitmap>>>,
    pub pending_thumbs: HashSet<u32>,
    pub rotation_scope: RotationScope,
    /// Session changes waiting to be persisted.
    pub dirty: bool,
    pub last_saved: f64,
    /// Time the document was opened (monotonic seconds).
    pub opened_at: f64,
    /// Render timing of the most recent page (milliseconds).
    pub last_render_ms: f32,
    /// True once the initial position has been restored.
    pub restored: bool,
    /// Reason the document failed to open, for the error banner.
    pub error: Option<String>,
}

impl DocView {
    pub fn new(
        id: DocId,
        identity: DocIdentity,
        row_id: Option<i64>,
        session: SessionRow,
        cache_limit: usize,
        now: f64,
    ) -> DocView {
        DocView {
            id,
            title: identity.display_name.clone(),
            identity,
            row_id,
            state: DocState::Loading,
            session,
            scroll: 0.0,
            layout: ViewLayout::default(),
            cache: PageCache::new(cache_limit),
            pending_renders: HashSet::new(),
            texts: HashMap::new(),
            pending_text: HashSet::new(),
            selection: None,
            highlights: Vec::new(),
            outline: Vec::new(),
            outline_requested: false,
            expanded: HashSet::new(),
            links: HashMap::new(),
            pending_links: HashSet::new(),
            search: SearchState::default(),
            rotation_scope: RotationScope::All,
            thumbs: HashMap::new(),
            pending_thumbs: HashSet::new(),
            dirty: false,
            last_saved: 0.0,
            opened_at: now,
            last_render_ms: 0.0,
            restored: false,
            error: None,
        }
    }

    pub fn info(&self) -> Option<&Arc<DocInfo>> {
        match &self.state {
            DocState::Ready(info) => Some(info),
            _ => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, DocState::Ready(_))
    }

    pub fn page_count(&self) -> u32 {
        self.info().map(|i| i.page_count).unwrap_or(0)
    }

    /// Zero-based index of the page currently filling the viewport.
    pub fn current_page(&self) -> u32 {
        self.layout.current_page()
    }

    /// Called when the document metadata arrives: restores the reading session.
    pub fn set_info(&mut self, info: Arc<DocInfo>, viewport: Size) {
        let mut session = self.session;
        session.clamp_values();
        session.last_page = session.last_page.clamp(1, info.page_count.max(1));
        self.session = session;
        self.state = DocState::Ready(info.clone());
        self.title = info
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| self.identity.display_name.clone());
        // Lay the document out and restore the reading position. The page in the
        // session is authoritative here: it is what the user last looked at.
        let anchor = session.last_page.saturating_sub(1);
        let first = layout::build_layout(&info, &session, viewport, 0.0);
        let scroll = first.scroll_for(anchor, session.scroll_frac);
        let mut placed = layout::build_layout(&info, &session, viewport, scroll);
        placed.scroll = scroll.min(placed.max_scroll());
        self.scroll = placed.scroll;
        self.layout = placed;
        self.restored = true;
        self.dirty = false;
    }

    /// Rebuilds page geometry for the current viewport.
    pub fn recompute_layout(&mut self, viewport: Size) {
        let Some(info) = self.info().cloned() else {
            return;
        };
        // In continuous mode the anchor page is derived from the scroll offset,
        // so keep the previously visible page to avoid feedback loops.
        let layout = layout::build_layout(&info, &self.session, viewport, self.scroll);
        let current = layout.current_page();
        self.scroll = layout.scroll;
        self.layout = layout;
        self.session.last_page = current + 1;
    }

    /// Scrolls by `delta` pixels, keeping the session page in sync.
    pub fn scroll_by(&mut self, delta: f32, viewport: Size) {
        if !self.is_ready() {
            return;
        }
        let max = self.layout.max_scroll();
        let target = (self.scroll + delta).clamp(0.0, max);
        if (target - self.scroll).abs() < 0.01 {
            return;
        }
        self.scroll = target;
        let page_before = self.session.last_page;
        self.recompute_layout(viewport);
        if self.session.last_page != page_before {
            self.dirty = true;
        } else {
            self.dirty = true;
        }
    }

    /// Jumps to a zero-based page, positioning its top at the viewport top.
    pub fn goto_page(&mut self, page: u32, viewport: Size) {
        if !self.is_ready() {
            return;
        }
        let count = self.page_count().max(1);
        let page = page.min(count - 1);
        self.session.last_page = page + 1;
        self.recompute_layout(viewport);
        self.scroll = self.layout.scroll_for(page, 0.0);
        self.recompute_layout(viewport);
        self.dirty = true;
    }

    pub fn set_zoom(&mut self, mode: ZoomMode, zoom: f32, viewport: Size) {
        if !self.is_ready() {
            return;
        }
        // Keep the reading position stable: remember the page and the fraction
        // of it that is above the viewport, then restore after relayout.
        let anchor_page = self.layout.current_page();
        let anchor_frac = self.layout.scroll_fraction(anchor_page);
        self.session.zoom_mode = mode;
        if mode == ZoomMode::Custom {
            self.session.zoom = zoom.clamp(layout::MIN_ZOOM, layout::MAX_ZOOM);
        }
        self.session.last_page = anchor_page + 1;
        self.recompute_layout(viewport);
        self.scroll = self.layout.scroll_for(anchor_page, anchor_frac);
        self.recompute_layout(viewport);
        self.dirty = true;
    }

    pub fn zoom_step(&mut self, in_: bool, viewport: Size) {
        let current = self.layout.zoom;
        let next = if in_ {
            layout::zoom_in(current)
        } else {
            layout::zoom_out(current)
        };
        self.set_zoom(ZoomMode::Custom, next, viewport);
    }

    /// Rotates. `clockwise = false` rotates counter-clockwise. When the scope is
    /// `Page` the rotation is stored per document anyway (the reader applies it
    /// to the visible page only); see `App::rotate` for the UI decision.
    pub fn rotate(&mut self, delta: i32, viewport: Size) {
        if !self.is_ready() {
            return;
        }
        let anchor_page = self.layout.current_page();
        let anchor_frac = self.layout.scroll_fraction(anchor_page);
        let next = (self.session.rotation as i32 + delta).rem_euclid(360) as u16;
        self.session.rotation = next;
        self.session.last_page = anchor_page + 1;
        self.cache.clear();
        self.pending_renders.clear();
        self.recompute_layout(viewport);
        self.scroll = self.layout.scroll_for(anchor_page, anchor_frac);
        self.recompute_layout(viewport);
        self.dirty = true;
    }

    pub fn set_reading_mode(&mut self, mode: ReadingMode, viewport: Size) {
        if !self.is_ready() || mode == self.session.reading_mode {
            return;
        }
        let page = self.layout.current_page();
        self.session.reading_mode = mode;
        self.session.last_page = page + 1;
        self.scroll = 0.0;
        self.recompute_layout(viewport);
        self.scroll = self.layout.scroll_for(page, 0.0);
        self.recompute_layout(viewport);
        self.dirty = true;
    }

    /// Rotation applied to a page when it is drawn: the page's own `/Rotate`
    /// plus the user's document rotation.
    pub fn effective_rotation(&self, page: u32) -> u16 {
        let intrinsic = self
            .info()
            .and_then(|i| i.page_rotations.get(page as usize).copied())
            .unwrap_or(0);
        (intrinsic + self.session.rotation) % 360
    }

    /// Converts a point given in page pixels from the page's top-left corner
    /// (what `ViewLayout::hit_test` returns) into PDF page coordinates (y up),
    /// honouring the page rotation.
    pub fn screen_to_pdf(&self, page: u32, local: Point) -> Option<Point> {
        let info = self.info()?;
        let base = info
            .page_sizes
            .get(page as usize)
            .copied()
            .unwrap_or(Size::new(595.0, 842.0));
        let display = crate::pdf::rotated(base, self.effective_rotation(page));
        let nx = (local.x / display.w.max(1.0)).clamp(0.0, 1.0);
        let ny = (local.y / display.h.max(1.0)).clamp(0.0, 1.0);
        let (ux, uy) = match self.effective_rotation(page) % 360 {
            90 => (ny, 1.0 - nx),
            180 => (1.0 - nx, 1.0 - ny),
            270 => (1.0 - ny, nx),
            _ => (nx, ny),
        };
        Some(Point::new(ux * base.w, (1.0 - uy) * base.h))
    }

    /// Converts a rectangle in PDF page coordinates (y up) into screen pixels
    /// inside `page_rect` (the on-screen rectangle of that page).
    pub fn pdf_to_screen(&self, page: u32, pdf: Rect, page_rect: Rect) -> Rect {
        let base = self
            .info()
            .and_then(|i| i.page_sizes.get(page as usize).copied())
            .unwrap_or(Size::new(595.0, 842.0));
        let rot = self.effective_rotation(page) % 360;
        let corner = |x: f32, y: f32| -> (f32, f32) {
            let ux = (x / base.w.max(1.0)).clamp(0.0, 1.0);
            let uy = (1.0 - y / base.h.max(1.0)).clamp(0.0, 1.0);
            let (nx, ny) = match rot {
                90 => (1.0 - uy, ux),
                180 => (1.0 - ux, 1.0 - uy),
                270 => (uy, 1.0 - ux),
                _ => (ux, uy),
            };
            (page_rect.x + nx * page_rect.w, page_rect.y + ny * page_rect.h)
        };
        let (x0, y0) = corner(pdf.x, pdf.y);
        let (x1, y1) = corner(pdf.x + pdf.w, pdf.y + pdf.h);
        Rect::new(
            x0.min(x1),
            y0.min(y1),
            (x0 - x1).abs().max(1.0),
            (y0 - y1).abs().max(1.0),
        )
    }

    /// Render key (page, zoom, rotation) for a page at the current zoom.
    pub fn render_key(&self, page: u32) -> RenderKey {
        RenderKey::new(page, self.layout.zoom, self.session.rotation)
    }

    /// Keys that should be resident for the current viewport.
    pub fn wanted_keys(&self) -> Vec<RenderKey> {
        let rotation = self.session.rotation;
        self.layout
            .visible_pages()
            .into_iter()
            .map(|page| RenderKey::new(page, self.layout.zoom, rotation))
            .collect()
    }

    /// Bitmap to draw for `page`: exact scale if cached, else the closest
    /// available scale (drawn scaled until the exact one arrives).
    pub fn bitmap_for(&mut self, page: u32) -> Option<(Arc<Bitmap>, bool)> {
        let key = self.render_key(page);
        if let Some(b) = self.cache.get(&key) {
            return Some((b, true));
        }
        self.cache
            .best_for(page, self.session.rotation)
            .map(|(_, b)| (b, false))
    }

    pub fn text(&self, page: u32) -> Option<&Arc<PageText>> {
        self.texts.get(&page)
    }

    /// Selection rectangles for `page` in PDF points (y up), for drawing and
    /// copying.
    pub fn selection_rects(&self, page: u32) -> Vec<Rect> {
        let Some(sel) = self.selection else {
            return Vec::new();
        };
        if sel.page != page || sel.is_empty() {
            return Vec::new();
        }
        let Some(text) = self.texts.get(&page) else {
            return Vec::new();
        };
        let (start, end) = sel.range();
        let mut rects: Vec<Rect> = Vec::new();
        for (i, c) in text.chars.iter().enumerate() {
            if i < start || i >= end {
                continue;
            }
            let r = Rect::new(c.left, c.bottom, c.width(), c.height());
            // Merge with the previous rect when they share a baseline and touch,
            // which keeps the overlay compact and avoids seams.
            if let Some(last) = rects.last_mut() {
                let same_line = (last.y - r.y).abs() < r.h.max(1.0) * 0.35;
                let touching = r.x - (last.x + last.w) < r.h.max(1.0) * 1.2 && r.x >= last.x - 1.0;
                if same_line && touching {
                    let right = (last.x + last.w).max(r.x + r.w);
                    last.w = right - last.x;
                    last.h = last.h.max(r.h);
                    continue;
                }
            }
            rects.push(r);
        }
        rects
    }

    /// Selected text, with newlines between lines.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let text = self.texts.get(&sel.page)?;
        let (start, end) = sel.range();
        if start >= end {
            return None;
        }
        let mut out = String::new();
        let mut last_bottom = f32::NAN;
        for (i, c) in text.chars.iter().enumerate() {
            if i < start || i >= end {
                continue;
            }
            if !last_bottom.is_nan() && (c.bottom - last_bottom).abs() > c.height().max(1.0) * 0.5 {
                out.push('\n');
            }
            out.push(c.ch);
            last_bottom = c.bottom;
        }
        if out.trim().is_empty() {
            None
        } else {
            Some(out)
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Highlight under a point in page coordinates, if any.
    pub fn highlight_at(&self, page: u32, point: Point) -> Option<i64> {
        for h in &self.highlights {
            if h.page != page {
                continue;
            }
            for r in &h.rects {
                if point.x >= r.x && point.x <= r.x + r.w && point.y >= r.y && point.y <= r.y + r.h {
                    return Some(h.id);
                }
            }
        }
        None
    }

    /// Link under a point in page coordinates.
    pub fn link_at(&self, page: u32, point: Point) -> Option<&LinkTarget> {
        self.links.get(&page).and_then(|links| {
            links
                .iter()
                .find(|l| {
                    point.x >= l.left
                        && point.x <= l.right
                        && point.y >= l.bottom
                        && point.y <= l.top
                })
                .map(|l| &l.target)
        })
    }

    /// Applies a fresh search result page from the worker.
    pub fn apply_search_hits(&mut self, page: u32, mut hits: Vec<SearchHit>) -> bool {
        if self.search.scanning_page != Some(page) {
            return false;
        }
        self.search.scanning_page = None;
        if hits.is_empty() {
            return false;
        }
        hits.sort_by_key(|h| h.char_index);
        let first_new = self.search.hits.len();
        let was_current_out_of_range = self.search.current >= self.search.hits.len();
        self.search.hits.extend(hits);
        if was_current_out_of_range {
            self.search.current = first_new;
        }
        true
    }

    /// Total memory used by this document's bitmaps.
    pub fn memory_bytes(&self) -> usize {
        let cache = self.cache.memory_bytes();
        let thumbs: usize = self
            .thumbs
            .values()
            .flatten()
            .map(|t| t.px.len() * 4)
            .sum();
        cache + thumbs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::cache::PageCache;
    use crate::doc::identity::DocIdentity;
    use crate::pdf::engine::{CharBox, PageText};

    fn identity(name: &str) -> DocIdentity {
        DocIdentity {
            fingerprint: format!("v1:{}", name),
            path: std::path::PathBuf::from(format!("/docs/{}", name)),
            display_name: name.to_string(),
            size: 100,
            modified: None,
        }
    }

    fn doc(pages: u32, viewport: Size) -> DocView {
        let mut view = DocView::new(
            1,
            identity("test.pdf"),
            None,
            SessionRow {
                zoom_mode: ZoomMode::Actual,
                ..Default::default()
            },
            64 * 1024 * 1024,
            0.0,
        );
        let info = DocInfo {
            page_count: pages,
            page_sizes: vec![Size::new(100.0, 200.0); pages as usize],
            page_rotations: vec![0; pages as usize],
            ..Default::default()
        };
        view.set_info(Arc::new(info), viewport);
        view
    }

    #[test]
    fn restoring_a_session_positions_the_scroll() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = DocView::new(
            1,
            identity("a.pdf"),
            None,
            SessionRow {
                last_page: 10,
                scroll_frac: 0.25,
                zoom_mode: ZoomMode::Actual,
                ..Default::default()
            },
            1 << 20,
            0.0,
        );
        let info = DocInfo {
            page_count: 20,
            page_sizes: vec![Size::new(100.0, 200.0); 20],
            page_rotations: vec![0; 20],
            ..Default::default()
        };
        view.set_info(Arc::new(info), viewport);
        assert_eq!(view.session.last_page, 10);
        let expected = view.layout.scroll_for(9, 0.25);
        assert!((view.scroll - expected).abs() < 1.0, "scroll={}", view.scroll);
        assert_eq!(view.current_page(), 9);
        assert!(view.restored);
    }

    #[test]
    fn goto_page_updates_the_session() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(20, viewport);
        view.goto_page(14, viewport);
        assert_eq!(view.session.last_page, 15);
        assert_eq!(view.current_page(), 14);
        assert!(view.dirty);
    }

    #[test]
    fn scrolling_updates_the_current_page() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(20, viewport);
        view.scroll_by(210.0 * 3.0, viewport);
        assert!(view.current_page() >= 2, "page={}", view.current_page());
    }

    #[test]
    fn scroll_is_clamped_to_the_document() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(3, viewport);
        view.scroll_by(-500.0, viewport);
        assert_eq!(view.scroll, 0.0);
        view.scroll_by(100_000.0, viewport);
        assert!((view.scroll - view.layout.max_scroll()).abs() < 0.5);
    }

    #[test]
    fn zooming_keeps_the_reading_position() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(20, viewport);
        view.goto_page(9, viewport);
        let page_before = view.current_page();
        let scroll_before = view.scroll;
        view.set_zoom(ZoomMode::Custom, 2.0, viewport);
        assert!((view.layout.zoom - 2.0).abs() < 1e-4);
        assert_eq!(view.current_page(), page_before);
        assert!(view.scroll > scroll_before);
    }

    #[test]
    fn rotation_swaps_page_dimensions() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(2, viewport);
        let before = view.layout.pages[0].w;
        view.rotate(90, viewport);
        assert_eq!(view.session.rotation, 90);
        assert!((view.layout.pages[0].w - 200.0).abs() < 0.5);
        assert!((before - 100.0).abs() < 0.5);
        view.rotate(-90, viewport);
        assert_eq!(view.session.rotation, 0);
    }

    #[test]
    fn selection_rects_merge_per_line() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(1, viewport);
        let chars = vec![
            CharBox { ch: 'a', left: 10.0, right: 20.0, bottom: 100.0, top: 110.0 },
            CharBox { ch: 'b', left: 20.0, right: 30.0, bottom: 100.0, top: 110.0 },
            CharBox { ch: 'c', left: 10.0, right: 20.0, bottom: 80.0, top: 90.0 },
        ];
        view.texts.insert(
            0,
            Arc::new(PageText {
                text: "abc".into(),
                chars,
            }),
        );
        view.selection = Some(Selection {
            page: 0,
            anchor: 0,
            focus: 3,
            dragging: false,
        });
        let rects = view.selection_rects(0);
        assert_eq!(rects.len(), 2, "{rects:?}");
        assert!((rects[0].w - 20.0).abs() < 0.01);
        assert_eq!(view.selected_text().as_deref(), Some("ab\nc"));
    }

    #[test]
    fn memory_accounting_covers_cache() {
        let viewport = Size::new(400.0, 300.0);
        let mut view = doc(1, viewport);
        view.cache
            .insert(RenderKey::new(0, 1.0, 0), Arc::new(Bitmap::filled(10, 10, crate::ui::canvas::Color::rgb(0, 0, 0))));
        assert_eq!(view.memory_bytes(), 400);
        let _ = PageCache::new(1);
    }
}
