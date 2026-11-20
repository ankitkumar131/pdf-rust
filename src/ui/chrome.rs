//! The reader chrome: toolbar, document tabs, sidebar panels, page rendering,
//! status bar, toasts and dialogs.
//!
//! Everything here is pure drawing plus deferred [`Action`]s — the chrome never
//! mutates application state, which keeps the borrow checker satisfied and makes
//! the UI straightforward to test.

use crate::app::dialogs::{Dialogs, ErrorDialog, FileDialog, GotoDialog, PasswordDialog, ResumePolicy, ResumeDialog};
use crate::app::docview::{DocState, DocView, RotationScope};
use crate::app::{Action, DialogKind, PerfStats, Regions, Settings, SidebarState, SidebarTab, Toast, ToastLevel};
use crate::db::{DocumentRow, ReadingMode, ZoomMode};
use crate::geom::{Point, Rect, Size};
use crate::ui::icons::{self, Icon};
use crate::ui::theme::Metrics;
use crate::ui::widgets::{self, TextField};
use crate::ui::{id, Color, Ui};

/// Everything the chrome needs, borrowed from [`crate::app::App`].
pub struct Chrome<'a> {
    pub docs: &'a mut Vec<DocView>,
    pub active: &'a mut usize,
    pub settings: &'a Settings,
    pub sidebar: &'a mut SidebarState,
    pub dialogs: &'a mut Dialogs,
    pub toasts: &'a [Toast],
    pub recent: &'a [DocumentRow],
    pub fullscreen: bool,
    pub presentation: bool,
    pub show_sidebar: bool,
    pub perf: &'a PerfStats,
    pub busy: &'a Option<String>,
    pub note: &'a Option<String>,
    pub engine_error: &'a Option<String>,
    pub db_error: &'a Option<String>,
    pub highlight_color: u32,
    pub regions: Regions,
    pub actions: &'a mut Vec<Action>,
    pub page_field: &'a mut TextField,
}

const BTN: f32 = 30.0;
const BTN_GAP: f32 = 4.0;

/// Cursor used to lay out a horizontal run of controls.
struct Row {
    x: f32,
    y: f32,
    h: f32,
}

impl Row {
    fn new(x: f32, y: f32, h: f32) -> Row {
        Row { x, y, h }
    }

    fn next(&mut self, w: f32) -> Rect {
        let r = Rect::new(self.x, self.y + (self.h - BTN) * 0.5, w, BTN);
        self.x += w + BTN_GAP;
        r
    }

    fn skip(&mut self, w: f32) {
        self.x += w;
    }
}

fn action(c: &mut Chrome, a: Action) {
    c.actions.push(a);
}

fn current_doc<'a>(c: &'a Chrome<'_>) -> Option<&'a DocView> {
    c.docs.get(*c.active)
}

/// Draws the whole frame.
pub fn paint(ui: &mut Ui, c: &mut Chrome) {
    if !c.presentation {
        paint_toolbar(ui, c);
        if c.regions.tabbar.h > 0.0 {
            paint_tabbar(ui, c);
        }
        if c.show_sidebar {
            paint_sidebar(ui, c);
        }
    }
    paint_content(ui, c);
    if !c.presentation {
        paint_status(ui, c);
    } else {
        paint_presentation_overlay(ui, c);
    }
    if c.dialogs.any() {
        // Dialogs must stay interactive even though the chrome underneath is
        // blocked while a modal is open.
        ui.state.modal = false;
        let ctx = DialogCtx {
            docs: &*c.docs,
            active: *c.active,
            settings: c.settings,
            highlight_color: c.highlight_color,
            db_error: c.db_error,
        };
        let mut out: Vec<Action> = Vec::new();
        paint_dialogs(ui, c.dialogs, &ctx, c.regions.window, &mut out);
        ui.state.modal = true;
        c.actions.extend(out);
    }
    paint_toasts(ui, c);
    // Paint the sidebar last so its scrollbars sit above the content edge.
}

// ----------------------------------------------------------------- toolbar

fn paint_toolbar(ui: &mut Ui, c: &mut Chrome) {
    let r = c.regions.toolbar;
    ui.bmp.fill_rect(r, ui.theme.toolbar_bg);
    ui.divider_h(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0));
    let has_doc = current_doc(c).map(|d| d.is_ready()).unwrap_or(false);
    let mut row = Row::new(8.0, r.y, r.h);
    let mut local: Vec<Action> = Vec::new();

    if widgets::icon_button(ui, id("tb-sidebar"), row.next(BTN), Icon::Menu, true, c.show_sidebar) {
        local.push(Action::ToggleSidebar);
    }
    if widgets::icon_button(ui, id("tb-open"), row.next(BTN), Icon::Open, true, false) {
        local.push(Action::Command(crate::app::commands::Command::OpenDialog));
    }
    row.skip(4.0);
    ui.divider_v(Rect::new(row.x, r.y + 9.0, 1.0, r.h - 18.0));
    row.skip(9.0);

    // Page navigation.
    if widgets::icon_button(ui, id("tb-prev"), row.next(BTN), Icon::Prev, has_doc, false) {
        local.push(Action::Command(crate::app::commands::Command::PrevPage));
    }
    let field_rect = Rect::new(row.x, r.y + (r.h - BTN) * 0.5, 74.0, BTN);
    row.skip(74.0 + BTN_GAP);
    let (page, total) = current_doc(c)
        .filter(|d| d.is_ready())
        .map(|d| (d.current_page() + 1, d.page_count()))
        .unwrap_or((0, 0));
    let focused = ui.state.focus == Some(id("page-field"));
    if !focused {
        *c.page_field = TextField::with_text(&format!("{}/{}", page, total));
    }
    let resp = widgets::text_field(ui, id("page-field"), field_rect, c.page_field, "1/1", true);
    if resp.submitted {
        if let Ok(n) = c.page_field.text.trim().parse::<u32>() {
            local.push(Action::GoToPage(n));
        }
        *c.page_field = TextField::with_text(&format!("{}/{}", page, total));
    }
    if widgets::icon_button(ui, id("tb-next"), row.next(BTN), Icon::Next, has_doc, false) {
        local.push(Action::Command(crate::app::commands::Command::NextPage));
    }
    row.skip(4.0);
    ui.divider_v(Rect::new(row.x, r.y + 9.0, 1.0, r.h - 18.0));
    row.skip(9.0);

    // Zoom.
    if widgets::icon_button(ui, id("tb-zoom-out"), row.next(BTN), Icon::ZoomOut, has_doc, false) {
        local.push(Action::Command(crate::app::commands::Command::ZoomOut));
    }
    let zoom_rect = row.next(60.0);
    let zoom = current_doc(c).map(|d| d.layout.zoom).unwrap_or(1.0);
    let label = format!("{:.0}%", zoom * 100.0);
    if widgets::button(ui, id("tb-zoom"), zoom_rect, &label, has_doc, false) {
        local.push(Action::SetFit(ZoomMode::FitPage));
    }
    if ui.hovered(zoom_rect) {
        ui.request_tooltip("Fit page (Ctrl+0)", zoom_rect.center());
    }
    if widgets::icon_button(ui, id("tb-zoom-in"), row.next(BTN), Icon::ZoomIn, has_doc, false) {
        local.push(Action::Command(crate::app::commands::Command::ZoomIn));
    }
    let fit_page = current_doc(c).map(|d| d.session.zoom_mode == ZoomMode::FitPage).unwrap_or(false);
    if widgets::icon_button(ui, id("tb-fit-page"), row.next(BTN), Icon::FitPage, has_doc, fit_page) {
        local.push(Action::SetFit(ZoomMode::FitPage));
    }
    let fit_width = current_doc(c).map(|d| d.session.zoom_mode == ZoomMode::FitWidth).unwrap_or(false);
    if widgets::icon_button(ui, id("tb-fit-width"), row.next(BTN), Icon::FitWidth, has_doc, fit_width) {
        local.push(Action::SetFit(ZoomMode::FitWidth));
    }
    row.skip(4.0);
    ui.divider_v(Rect::new(row.x, r.y + 9.0, 1.0, r.h - 18.0));
    row.skip(9.0);

    // Rotation and reading mode.
    if widgets::icon_button(ui, id("tb-rot-ccw"), row.next(BTN), Icon::RotateCcw, has_doc, false) {
        local.push(Action::SetRotation(-90));
    }
    if widgets::icon_button(ui, id("tb-rot-cw"), row.next(BTN), Icon::RotateCw, has_doc, false) {
        local.push(Action::SetRotation(90));
    }
    if widgets::icon_button(ui, id("tb-rot-180"), row.next(BTN), Icon::Rotate180, has_doc, false) {
        local.push(Action::SetRotation(180));
    }
    let scope = current_doc(c).map(|d| d.rotation_scope).unwrap_or(RotationScope::All);
    let scope_rect = row.next(78.0);
    if widgets::button(
        ui,
        id("tb-rot-scope"),
        scope_rect,
        match scope {
            RotationScope::All => "All pages",
            RotationScope::Page => "This page",
        },
        has_doc,
        false,
    ) {
        local.push(Action::SetRotationScope(match scope {
            RotationScope::All => RotationScope::Page,
            RotationScope::Page => RotationScope::All,
        }));
    }
    if ui.hovered(scope_rect) {
        ui.request_tooltip("Rotate every page or only the current page", scope_rect.center());
    }
    let mode = current_doc(c)
        .map(|d| d.session.reading_mode)
        .unwrap_or(ReadingMode::Continuous);
    let mode_rect = Rect::new(row.x, r.y + (r.h - BTN) * 0.5, 120.0, BTN);
    row.skip(120.0 + BTN_GAP);
    if widgets::icon_text_button(
        ui,
        id("tb-mode"),
        mode_rect,
        Icon::Menu,
        match mode {
            ReadingMode::Continuous => "Continuous",
            ReadingMode::Single => "Single page",
        },
        has_doc,
        false,
    ) {
        local.push(Action::SetReadingMode(match mode {
            ReadingMode::Continuous => ReadingMode::Single,
            ReadingMode::Single => ReadingMode::Continuous,
        }));
    }

    // Right-hand side controls, laid out from the right edge.
    let mut x = r.right() - 8.0 - BTN;
    let mut slots = [Rect::ZERO; 8];
    for slot in slots.iter_mut() {
        *slot = Rect::new(x, r.y + (r.h - BTN) * 0.5, BTN, BTN);
        x -= BTN + BTN_GAP;
    }
    if widgets::icon_button(ui, id("tb-search"), slots[0], Icon::Search, true, c.show_sidebar && c.sidebar.tab == SidebarTab::Search) {
        local.push(Action::SetSidebarTab(SidebarTab::Search));
    }
    if widgets::icon_button(ui, id("tb-print"), slots[1], Icon::Print, has_doc, false) {
        local.push(Action::PrintDocument);
    }
    if widgets::icon_button(ui, id("tb-fullscreen"), slots[2], Icon::Fullscreen, true, c.fullscreen) {
        local.push(Action::SetFullscreen(!c.fullscreen));
    }
    if widgets::icon_button(ui, id("tb-present"), slots[3], Icon::Presentation, has_doc, c.presentation) {
        local.push(Action::SetPresentation(!c.presentation));
    }
    let theme_icon = if c.settings.dark { Icon::Sun } else { Icon::Moon };
    if widgets::icon_button(ui, id("tb-theme"), slots[4], theme_icon, true, false) {
        local.push(Action::ToggleTheme);
    }
    if widgets::icon_button(ui, id("tb-settings"), slots[5], Icon::Settings, true, false) {
        local.push(Action::ShowDialog(DialogKind::Settings));
    }
    if widgets::icon_button(ui, id("tb-info"), slots[6], Icon::Info, true, false) {
        local.push(Action::ShowDialog(DialogKind::Info));
    }
    if widgets::icon_button(ui, id("tb-close"), slots[7], Icon::Close, true, false) {
        local.push(Action::Quit);
    }
    c.actions.extend(local);
}

// ------------------------------------------------------------------ tab bar

fn paint_tabbar(ui: &mut Ui, c: &mut Chrome) {
    let r = c.regions.tabbar;
    ui.bmp.fill_rect(r, ui.theme.window_bg);
    ui.divider_h(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0));
    let mut x = 6.0;
    let mut requested: Option<usize> = None;
    let mut closed: Option<usize> = None;
    let mut tooltip: Option<(String, Point)> = None;
    for (i, doc) in c.docs.iter().enumerate() {
        let title = doc.title.clone();
        let w = (ui.font.text_width(&title, Metrics::TEXT_SIZE) + 62.0).clamp(96.0, 240.0);
        if x + w > r.right() - 10.0 {
            break;
        }
        let tab = Rect::new(x, r.y + 3.0, w, r.h - 4.0);
        let selected = i == *c.active;
        if widgets::tab(ui, id(&format!("tab-{}", i)), tab, &title, selected) {
            requested = Some(i);
        }
        let close = Rect::new(tab.right() - 22.0, tab.y + (tab.h - 16.0) * 0.5, 16.0, 16.0);
        if widgets::icon_button(ui, id(&format!("tab-close-{}", i)), close, Icon::Close, true, false) {
            closed = Some(i);
        }
        if ui.hovered(tab) {
            tooltip = Some((doc.identity.path.to_string_lossy().into_owned(), tab.center()));
        }
        x += w + 3.0;
    }
    if let Some((text, at)) = tooltip {
        ui.request_tooltip(&text, at);
    }
    if let Some(i) = requested {
        action(c, Action::ActivateDoc(i));
    }
    if let Some(i) = closed {
        action(c, Action::CloseDoc(i));
    }
}

// ------------------------------------------------------------------ sidebar

fn paint_sidebar(ui: &mut Ui, c: &mut Chrome) {
    let r = c.regions.sidebar;
    if r.is_empty() {
        return;
    }
    ui.bmp.fill_rect(r, ui.theme.sidebar_bg);
    ui.divider_v(Rect::new(r.right() - 1.0, r.y, 1.0, r.h));

    let tabs = Rect::new(r.x, r.y, r.w - 1.0, 34.0);
    let tab_w = tabs.w / 4.0;
    let mut picked: Option<SidebarTab> = None;
    for (i, tab) in [
        SidebarTab::Thumbnails,
        SidebarTab::Outline,
        SidebarTab::Search,
        SidebarTab::Recent,
    ]
    .into_iter()
    .enumerate()
    {
        let cell = Rect::new(tabs.x + i as f32 * tab_w, tabs.y + 3.0, tab_w, tabs.h - 6.0);
        let icon = match tab {
            SidebarTab::Thumbnails => Icon::Thumbnails,
            SidebarTab::Outline => Icon::Outline,
            SidebarTab::Search => Icon::Search,
            SidebarTab::Recent => Icon::Recent,
        };
        if widgets::icon_button(ui, id(&format!("side-tab-{}", i)), cell, icon, true, c.sidebar.tab == tab) {
            picked = Some(tab);
        }
        if ui.hovered(cell) {
            ui.request_tooltip(tab.label(), cell.center());
        }
    }
    if let Some(tab) = picked {
        action(c, Action::SetSidebarTab(tab));
    }
    ui.divider_h(Rect::new(r.x, tabs.bottom(), r.w, 1.0));
    let body = Rect::new(r.x, tabs.bottom() + 1.0, r.w - 1.0, r.h - tabs.h - 1.0);

    match c.sidebar.tab {
        SidebarTab::Thumbnails => paint_thumbnails(ui, c, body),
        SidebarTab::Outline => paint_outline(ui, c, body),
        SidebarTab::Search => paint_search(ui, c, body),
        SidebarTab::Recent => paint_recent(ui, c, body),
    }

    // Drag the right edge to resize the sidebar.
    let handle = Rect::new(r.right() - 5.0, r.y, 5.0, r.h);
    let dragging = ui.state.dragging() == Some(id("sidebar-resize"));
    if !dragging {
        ui.interact(id("sidebar-resize"), handle, true);
    }
    if ui.state.dragging() == Some(id("sidebar-resize")) && ui.input.down {
        action(c, Action::SetSidebarWidth(ui.input.mouse.x));
    }
}

fn paint_thumbnails(ui: &mut Ui, c: &mut Chrome, body: Rect) {
    let Some(doc) = current_doc(c) else {
        empty_sidebar(ui, body, "No document open", "Open a PDF to see thumbnails");
        return;
    };
    if !doc.is_ready() {
        empty_sidebar(ui, body, "Loading…", "Thumbnails appear once the document opens");
        return;
    }
    let total = doc.page_count();
    let current = doc.current_page();
    let rotation = doc.session.rotation;
    let pad = 10.0;
    let cell_w = (body.w - pad * 2.0 - Metrics::SCROLLBAR_W).max(60.0);
    let mut offset = c.sidebar.thumb_scroll;
    let mut clicked: Option<u32> = None;
    let mut y = body.y + 8.0 - offset;
    let mut content_h = 12.0;
    for page in 0..total {
        let size = doc
            .info()
            .map(|i| i.display_size(page, rotation))
            .unwrap_or(Size::new(595.0, 842.0));
        let thumb_h = (cell_w * size.h / size.w.max(1.0)).clamp(40.0, 400.0);
        let cell = Rect::new(body.x + pad, y, cell_w, thumb_h);
        content_h += thumb_h + 24.0;
        if cell.bottom() > body.y && cell.y < body.bottom() {
            let selected = page == current;
            let id_cell = id(&format!("thumb-{}", page));
            if ui.interact(id_cell, cell, true) {
                clicked = Some(page);
            }
            if selected {
                ui.bmp.fill_rect(cell.inset_xy(-2.0, -2.0), ui.theme.accent_soft);
            } else if ui.hovered(cell) {
                ui.bmp.fill_rect(cell.inset_xy(-2.0, -2.0), ui.theme.control_hover);
            }
            let thumb_area = cell.inset_xy(2.0, 2.0);
            match doc.thumbs.get(&page) {
                Some(Some(bmp)) => {
                    let dst = fit_inside(thumb_area, bmp.width as f32, bmp.height as f32);
                    let shadow = ui.theme.shadow.fade(0.30);
                    ui.bmp
                        .fill_rect(Rect::new(dst.right(), dst.y + 2.0, 3.0, dst.h), shadow);
                    ui.bmp
                        .fill_rect(Rect::new(dst.x + 2.0, dst.bottom(), dst.w + 1.0, 3.0), shadow);
                    ui.bmp.fill_rect(dst, Color::rgb(255, 255, 255));
                    ui.bmp.blit_scaled(bmp, dst, 1.0);
                    ui.bmp.stroke_round_rect(dst, 1.0, ui.theme.page_border, 1.0);
                }
                _ => {
                    ui.bmp.fill_rect(thumb_area, ui.theme.control_bg);
                    ui.bmp.stroke_round_rect(thumb_area, 2.0, ui.theme.control_border, 1.0);
                    ui.spinner(thumb_area.center(), 6.0);
                }
            }
            if selected {
                ui.bmp.stroke_round_rect(
                    Rect::new(cell.x - 2.0, cell.y - 2.0, cell.w + 4.0, cell.h + 4.0),
                    5.0,
                    ui.theme.accent,
                    1.5,
                );
            }
            let label = format!("{}", page + 1);
            ui.font.draw_text_centered(
                ui.bmp,
                Rect::new(cell.x, cell.bottom() + 3.0, cell.w, 16.0),
                Metrics::SMALL_TEXT_SIZE,
                if selected { ui.theme.accent } else { ui.theme.text_muted },
                &label,
            );
        }
        y += thumb_h + 24.0;
    }
    let mut changed = widgets::scrollbar(
        ui,
        id("thumb-scroll"),
        Rect::new(body.right() - Metrics::SCROLLBAR_W - 4.0, body.y + 4.0, Metrics::SCROLLBAR_W, body.h - 8.0),
        content_h,
        body.h,
        &mut offset,
    );
    // Follow the current page when it changes.
    if c.sidebar.thumb_focus != current {
        c.sidebar.thumb_focus = current;
        let per_page = content_h / total.max(1) as f32;
        let target = (current as f32 * per_page - body.h * 0.4).max(0.0);
        offset = target.min((content_h - body.h).max(0.0));
        changed = true;
    }
    if changed || offset != c.sidebar.thumb_scroll {
        c.sidebar.thumb_scroll = offset;
    }
    if let Some(page) = clicked {
        action(c, Action::GoToPage(page + 1));
    }
}

/// What the user clicked inside the outline panel.
enum OutlineClick {
    Toggle(String),
    Goto(u32),
    Uri(String),
}

fn paint_outline(ui: &mut Ui, c: &mut Chrome, body: Rect) {
    let Some(doc) = current_doc(c) else {
        empty_sidebar(ui, body, "No document open", "Open a PDF to see its contents");
        return;
    };
    if doc.outline.is_empty() {
        if doc.outline_requested {
            empty_sidebar(ui, body, "Reading contents…", "The outline appears in a moment");
        } else {
            empty_sidebar(ui, body, "No contents", "This document has no table of contents");
        }
        return;
    }
    let rows = flatten_outline(&doc.outline, 0.0, &doc.expanded);
    let row_h = 26.0;
    let mut offset = c.sidebar.outline_scroll;
    let content_h = rows.len() as f32 * row_h + 8.0;
    let mut y = body.y + 4.0 - offset;
    let mut clicked: Option<OutlineClick> = None;
    for (i, (indent, item)) in rows.iter().enumerate() {
        let r = Rect::new(body.x + 2.0, y, body.w - Metrics::SCROLLBAR_W - 6.0, row_h);
        if r.bottom() > body.y && r.y < body.bottom() {
            let label = if item.title.is_empty() {
                "(untitled)".to_string()
            } else {
                item.title.clone()
            };
            let has_children = !item.children.is_empty();
            let expanded = doc.expanded.contains(&item.title);
            if has_children {
                let chevron = Rect::new(r.x + 4.0 + indent * 14.0, r.y + 5.0, 16.0, 16.0);
                let icon = if expanded { Icon::ChevronDown } else { Icon::Next };
                if widgets::icon_button(ui, id(&format!("ol-toggle-{}", i)), chevron, icon, true, false) {
                    clicked = Some(OutlineClick::Toggle(item.title.clone()));
                }
            }
            let row_rect = Rect::new(r.x + 22.0, r.y, r.w - 22.0, r.h);
            if widgets::list_row(ui, id(&format!("ol-{}", i)), row_rect, &label, None, false, indent * 14.0)
                && clicked.is_none()
            {
                clicked = Some(if let Some(page) = item.page {
                    OutlineClick::Goto(page)
                } else if let Some(uri) = &item.uri {
                    OutlineClick::Uri(uri.clone())
                } else if has_children {
                    OutlineClick::Toggle(item.title.clone())
                } else {
                    OutlineClick::Goto(0)
                });
            }
            if ui.hovered(r) && ui.font.text_width(&label, Metrics::TEXT_SIZE) > r.w - 34.0 - indent * 14.0 {
                ui.request_tooltip(&label, Point::new(r.x + 24.0, r.y));
            }
        }
        y += row_h;
    }
    let changed = widgets::scrollbar(
        ui,
        id("outline-scroll"),
        Rect::new(body.right() - Metrics::SCROLLBAR_W - 4.0, body.y + 4.0, Metrics::SCROLLBAR_W, body.h - 8.0),
        content_h,
        body.h,
        &mut offset,
    );
    if changed {
        c.sidebar.outline_scroll = offset;
    }
    match clicked {
        Some(OutlineClick::Goto(page)) => action(c, Action::GoToPage(page + 1)),
        Some(OutlineClick::Uri(uri)) => action(c, Action::OpenUrl(uri)),
        Some(OutlineClick::Toggle(title)) => action(c, Action::ToggleOutline(title)),
        None => {}
    }
}

fn flatten_outline(
    items: &[crate::pdf::OutlineItem],
    depth: f32,
    expanded: &std::collections::HashSet<String>,
) -> Vec<(f32, crate::pdf::OutlineItem)> {
    let mut out = Vec::new();
    for item in items {
        out.push((depth, item.clone()));
        if !item.children.is_empty() && expanded.contains(&item.title) {
            out.extend(flatten_outline(&item.children, depth + 1.0, expanded));
        }
    }
    out
}

fn paint_search(ui: &mut Ui, c: &mut Chrome, body: Rect) {
    let has_doc = current_doc(c).map(|d| d.is_ready()).unwrap_or(false);
    let mut local: Vec<Action> = Vec::new();
    let mut y = body.y + 8.0;
    let field = Rect::new(body.x + 8.0, y, body.w - 16.0, 30.0);
    let resp = widgets::text_field(
        ui,
        id("search-field"),
        field,
        &mut c.sidebar.search_field,
        "Search document",
        false,
    );
    if resp.changed {
        local.push(Action::SearchQuery(c.sidebar.search_field.text.clone()));
    }
    if resp.submitted {
        local.push(Action::SearchNext);
    }
    y += 38.0;
    let mut row = Row::new(body.x + 8.0, y, BTN);
    if widgets::icon_button(ui, id("search-prev"), row.next(BTN), Icon::PrevMatch, has_doc, false) {
        local.push(Action::SearchPrev);
    }
    if widgets::icon_button(ui, id("search-next"), row.next(BTN), Icon::NextMatch, has_doc, false) {
        local.push(Action::SearchNext);
    }
    let match_case = current_doc(c).map(|d| d.search.match_case).unwrap_or(false);
    let case_rect = Rect::new(row.x, y + 3.0, 130.0, 24.0);
    if widgets::checkbox(ui, id("search-case"), case_rect, "Match case", match_case) {
        local.push(Action::ToggleMatchCase);
    }
    y += 38.0;
    let (state, current) = current_doc(c)
        .map(|d| (d.search.clone(), d.search.current))
        .unwrap_or_default();
    if state.query.trim().is_empty() {
        empty_sidebar(
            ui,
            Rect::new(body.x, y, body.w, body.bottom() - y),
            "Search the document",
            "Ctrl+F focuses this field · F3 goes to the next match",
        );
        c.actions.extend(local);
        return;
    }
    let summary = if state.hits.is_empty() {
        if state.no_more_pages {
            "No matches".to_string()
        } else {
            "Searching…".to_string()
        }
    } else {
        format!("{} match(es) · {} of {}", state.hits.len(), current + 1, state.hits.len())
    };
    ui.font.draw_text(
        ui.bmp,
        body.x + 10.0,
        y + ui.font.ascent(Metrics::SMALL_TEXT_SIZE) + 2.0,
        Metrics::SMALL_TEXT_SIZE,
        ui.theme.text_muted,
        &summary,
    );
    y += 18.0;
    let list = Rect::new(body.x, y, body.w, (body.bottom() - y).max(20.0));
    let row_h = 34.0;
    let mut offset = c.sidebar.search_scroll;
    let content_h = state.hits.len() as f32 * row_h + 4.0;
    let mut hit_clicked: Option<usize> = None;
    let mut ry = list.y + 2.0 - offset;
    let texts = current_doc(c).map(|d| d.texts.clone()).unwrap_or_default();
    for (i, hit) in state.hits.iter().enumerate() {
        let r = Rect::new(list.x + 4.0, ry, list.w - Metrics::SCROLLBAR_W - 8.0, row_h - 2.0);
        if r.bottom() > list.y && r.y < list.bottom() {
            let snippet = texts
                .get(&hit.page)
                .map(|t| snippet_for(t, hit.char_index as usize, hit.char_count as usize))
                .unwrap_or_default();
            let subtitle = format!("Page {}", hit.page + 1);
            if widgets::list_row(
                ui,
                id(&format!("hit-{}", i)),
                r,
                &snippet,
                Some(&subtitle),
                i == current,
                0.0,
            ) {
                hit_clicked = Some(i);
            }
        }
        ry += row_h;
    }
    let changed = widgets::scrollbar(
        ui,
        id("search-scroll"),
        Rect::new(list.right() - Metrics::SCROLLBAR_W - 2.0, list.y, Metrics::SCROLLBAR_W, list.h),
        content_h,
        list.h,
        &mut offset,
    );
    if changed {
        c.sidebar.search_scroll = offset;
    }
    if let Some(i) = hit_clicked {
        local.push(Action::GotoSearchHit(i));
    }
    c.actions.extend(local);
}

fn snippet_for(text: &crate::pdf::PageText, start: usize, len: usize) -> String {
    let chars: Vec<char> = text.text.chars().collect();
    let from = start.saturating_sub(28);
    let to = (start + len + 34).min(chars.len());
    let mut s = String::new();
    if from > 0 {
        s.push('…');
    }
    s.extend(chars.get(from..to).unwrap_or(&[]).iter());
    if to < chars.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

fn paint_recent(ui: &mut Ui, c: &mut Chrome, body: Rect) {
    if c.recent.is_empty() {
        empty_sidebar(ui, body, "No recent documents", "Documents you open show up here");
        return;
    }
    let row_h = 44.0;
    let mut offset = c.sidebar.recent_scroll;
    let content_h = c.recent.len() as f32 * row_h + 8.0;
    let mut y = body.y + 4.0 - offset;
    let mut open: Option<std::path::PathBuf> = None;
    let mut forget: Option<i64> = None;
    let mut tooltip: Option<(String, Point)> = None;
    for (i, row) in c.recent.iter().enumerate() {
        let r = Rect::new(body.x + 4.0, y, body.w - Metrics::SCROLLBAR_W - 10.0, row_h - 4.0);
        if r.bottom() > body.y && r.y < body.bottom() {
            let title = display_title(row);
            let subtitle = format!(
                "Page {} · {}",
                row.session.last_page,
                std::path::Path::new(&row.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            if widgets::list_row(ui, id(&format!("recent-{}", i)), r, &title, Some(&subtitle), false, 0.0) {
                open = Some(std::path::PathBuf::from(&row.path));
            }
            let x = Rect::new(r.right() - 22.0, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0);
            if widgets::icon_button(ui, id(&format!("recent-x-{}", i)), x, Icon::Close, true, false) {
                forget = Some(row.id);
            }
            if ui.hovered(r) {
                tooltip = Some((row.path.clone(), Point::new(r.x + 12.0, r.y)));
            }
        }
        y += row_h;
    }
    if let Some((text, at)) = tooltip {
        ui.request_tooltip(&text, at);
    }
    let changed = widgets::scrollbar(
        ui,
        id("recent-scroll"),
        Rect::new(body.right() - Metrics::SCROLLBAR_W - 4.0, body.y + 4.0, Metrics::SCROLLBAR_W, body.h - 8.0),
        content_h,
        body.h,
        &mut offset,
    );
    if changed {
        c.sidebar.recent_scroll = offset;
    }
    if let Some(path) = open {
        action(c, Action::OpenPath(path));
    }
    if let Some(row_id) = forget {
        action(c, Action::ForgetDocument(row_id));
    }
}

fn display_title(row: &DocumentRow) -> String {
    row.title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            std::path::Path::new(&row.path)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| row.path.clone())
        })
}

fn empty_sidebar(ui: &mut Ui, body: Rect, title: &str, hint: &str) {
    ui.font.draw_text_centered(
        ui.bmp,
        Rect::new(body.x, body.y + 16.0, body.w, 20.0),
        Metrics::TEXT_SIZE,
        ui.theme.text_muted,
        title,
    );
    let text = ui.font.ellipsize(hint, Metrics::SMALL_TEXT_SIZE, body.w - 24.0).into_owned();
    ui.font.draw_text_centered(
        ui.bmp,
        Rect::new(body.x + 12.0, body.y + 38.0, body.w - 24.0, 18.0),
        Metrics::SMALL_TEXT_SIZE,
        ui.theme.text_disabled,
        &text,
    );
}

fn fit_inside(area: Rect, src_w: f32, src_h: f32) -> Rect {
    if src_w <= 0.0 || src_h <= 0.0 {
        return area;
    }
    let scale = (area.w / src_w).min(area.h / src_h);
    let w = (src_w * scale).min(area.w).max(1.0);
    let h = (src_h * scale).min(area.h).max(1.0);
    Rect::new(area.x + (area.w - w) * 0.5, area.y + (area.h - h) * 0.5, w, h)
}

// ------------------------------------------------------------------ content

fn paint_content(ui: &mut Ui, c: &mut Chrome) {
    let content = c.regions.content;
    ui.bmp.fill_rect(content, ui.theme.viewer_bg);

    if c.docs.is_empty() {
        paint_welcome(ui, c, content);
        return;
    }
    let index = *c.active;
    let Some(doc) = c.docs.get_mut(index) else {
        return;
    };
    match doc.state.clone() {
        DocState::Loading => {
            ui.spinner(content.center(), 12.0);
            ui.font.draw_text_centered(
                ui.bmp,
                Rect::new(content.x, content.center().y + 18.0, content.w, 22.0),
                Metrics::TEXT_SIZE,
                ui.theme.text_muted,
                "Opening document…",
            );
            return;
        }
        DocState::NeedsPassword => {
            message_panel(ui, content, "Password required", "Enter the document password to open it.");
            return;
        }
        DocState::Failed(err) => {
            message_panel(ui, content, "Cannot open this document", &err);
            return;
        }
        DocState::Ready(_) => {}
    }
    if doc.layout.pages.is_empty() {
        return;
    }
    let rotation = doc.session.rotation;
    let zoom = doc.layout.zoom;
    let _ = zoom;
    let page_count = doc.layout.pages.len();
    let pages: Vec<crate::app::layout::PageGeom> = doc.layout.pages.clone();
    for geom in &pages {
        let rect = doc.layout.page_screen_rect(geom, content);
        if rect.bottom() < content.y || rect.y > content.bottom() {
            continue;
        }
        // Shadow along the two visible edges only: filling a full translucent
        // rectangle behind every page costs more than blitting the page itself.
        let shadow = ui.theme.shadow.fade(0.30);
        ui.bmp
            .fill_rect(Rect::new(rect.right(), rect.y + 3.0, 5.0, rect.h), shadow);
        ui.bmp
            .fill_rect(Rect::new(rect.x + 3.0, rect.bottom(), rect.w + 2.0, 5.0), shadow);
        match doc.bitmap_for(geom.page) {
            Some((bmp, _exact)) => {
                ui.bmp.blit_scaled(&bmp, rect, 1.0);
            }
            None => {
                ui.bmp.fill_rect(rect, Color::rgb(255, 255, 255));
                if doc.pending_renders.iter().any(|k| k.page == geom.page) {
                    ui.spinner(rect.center(), 10.0);
                }
            }
        }
        ui.bmp.stroke_round_rect(rect, 1.0, ui.theme.page_border, 1.0);
        paint_page_overlays(ui, doc, geom.page, rect, zoom, rotation);
        if page_count > 1 {
            let label = format!("{}", geom.page + 1);
            ui.font.draw_text_centered(
                ui.bmp,
                Rect::new(rect.x, rect.bottom() + 3.0, rect.w, 16.0),
                Metrics::SMALL_TEXT_SIZE,
                ui.theme.text_muted,
                &label,
            );
        }
    }

    // Vertical scrollbar for the document.
    let mut scroll = doc.scroll;
    let changed = widgets::scrollbar(
        ui,
        id("page-scroll"),
        Rect::new(
            content.right() - Metrics::SCROLLBAR_W - 2.0,
            content.y + 2.0,
            Metrics::SCROLLBAR_W,
            content.h - 4.0,
        ),
        doc.layout.total_h,
        content.h,
        &mut scroll,
    );
    if changed {
        doc.scroll = scroll;
        doc.recompute_layout(content.size());
        doc.dirty = true;
    }

    if !doc.pending_renders.is_empty() {
        ui.spinner(Point::new(content.right() - 26.0, content.y + 22.0), 7.0);
    }
}

fn paint_page_overlays(ui: &mut Ui, doc: &DocView, page: u32, rect: Rect, _zoom: f32, _rotation: u16) {
    for h in &doc.highlights {
        if h.page != page {
            continue;
        }
        let color = Color::from_argb(h.color).fade(0.55);
        for r in &h.rects {
            ui.bmp.fill_rect(doc.pdf_to_screen(page, *r, rect), color);
        }
    }
    for (i, hit) in doc.search.hits.iter().enumerate() {
        if hit.page != page {
            continue;
        }
        let active = match doc.search.current_hit() {
            Some(current) => current.char_index == hit.char_index && current.page == hit.page,
            None => i == doc.search.current,
        };
        let color = if active { ui.theme.search_hit_active } else { ui.theme.search_hit };
        ui.bmp.fill_rect(
            doc.pdf_to_screen(
                page,
                Rect::new(hit.left, hit.bottom, hit.right - hit.left, hit.top - hit.bottom),
                rect,
            ),
            color,
        );
    }
    for r in doc.selection_rects(page) {
        ui.bmp
            .fill_rect(doc.pdf_to_screen(page, r, rect), ui.theme.selection);
    }
}

fn paint_welcome(ui: &mut Ui, c: &mut Chrome, content: Rect) {
    let theme = ui.theme;
    let w = content.w.min(560.0);
    let x = content.x + (content.w - w) * 0.5;
    let mut y = content.y + (content.h * 0.16).max(16.0);
    ui.font.draw_text(ui.bmp, x, y + 30.0, Metrics::TITLE_TEXT_SIZE + 12.0, theme.text, "pdf-rust");
    y += 38.0;
    ui.font.draw_text(
        ui.bmp,
        x,
        y + 16.0,
        Metrics::TEXT_SIZE,
        theme.text_muted,
        "A small, fast, native PDF reader.",
    );
    y += 38.0;
    let mut local: Vec<Action> = Vec::new();
    let open = Rect::new(x, y, 150.0, 34.0);
    if widgets::button(ui, id("welcome-open"), open, "Open PDF…", true, true) {
        local.push(Action::Command(crate::app::commands::Command::OpenDialog));
    }
    let keys = Rect::new(open.right() + 10.0, y, 160.0, 34.0);
    if widgets::button(ui, id("welcome-keys"), keys, "Keyboard shortcuts", true, false) {
        local.push(Action::ShowShortcuts(true));
    }
    y += 52.0;
    if let Some(err) = c.engine_error {
        let box_r = Rect::new(x, y, w, 64.0);
        ui.bmp.fill_round_rect(box_r, 6.0, theme.error.fade(0.15));
        ui.bmp.stroke_round_rect(box_r, 6.0, theme.error, 1.0);
        ui.font.draw_text(
            ui.bmp,
            box_r.x + 12.0,
            box_r.y + 22.0,
            Metrics::TEXT_SIZE,
            theme.error,
            "The PDF engine could not be loaded",
        );
        let text = ui.font.ellipsize(err, Metrics::SMALL_TEXT_SIZE, box_r.w - 24.0).into_owned();
        ui.font.draw_text(
            ui.bmp,
            box_r.x + 12.0,
            box_r.y + 43.0,
            Metrics::SMALL_TEXT_SIZE,
            theme.text_muted,
            &text,
        );
        y += 76.0;
    }
    if let Some(err) = c.db_error {
        let text = ui
            .font
            .ellipsize(&format!("History is unavailable: {}", err), Metrics::SMALL_TEXT_SIZE, w)
            .into_owned();
        ui.font.draw_text(ui.bmp, x, y + 12.0, Metrics::SMALL_TEXT_SIZE, theme.warning, &text);
        y += 26.0;
    }
    if !c.recent.is_empty() {
        widgets::section_label(ui, Rect::new(x, y, w, 18.0), "RECENT DOCUMENTS");
        y += 24.0;
        for (i, row) in c.recent.iter().take(6).enumerate() {
            let r = Rect::new(x, y, w, 30.0);
            let title = display_title(row);
            let subtitle = format!("Page {}", row.session.last_page);
            if widgets::list_row(ui, id(&format!("welcome-recent-{}", i)), r, &title, Some(&subtitle), false, 0.0) {
                local.push(Action::OpenPath(std::path::PathBuf::from(&row.path)));
            }
            y += 32.0;
        }
    }
    let hint = "Ctrl+O open · Ctrl+F search · F9 sidebar · F11 full screen · Ctrl+Q quit";
    ui.font.draw_text(
        ui.bmp,
        x,
        content.bottom() - 16.0,
        Metrics::SMALL_TEXT_SIZE,
        theme.text_disabled,
        hint,
    );
    c.actions.extend(local);
}

fn message_panel(ui: &mut Ui, content: Rect, title: &str, message: &str) {
    let w = content.w.min(460.0);
    let y = content.y + content.h * 0.38;
    ui.font.draw_text_centered(
        ui.bmp,
        Rect::new(content.x + (content.w - w) * 0.5, y, w, 24.0),
        Metrics::TITLE_TEXT_SIZE,
        ui.theme.text,
        title,
    );
    for (i, line) in wrap(message, 60).into_iter().enumerate() {
        let text = ui.font.ellipsize(&line, Metrics::TEXT_SIZE, w).into_owned();
        ui.font.draw_text_centered(
            ui.bmp,
            Rect::new(content.x + (content.w - w) * 0.5, y + 26.0 + i as f32 * 18.0, w, 20.0),
            Metrics::TEXT_SIZE,
            ui.theme.text_muted,
            &text,
        );
        if i >= 4 {
            break;
        }
    }
}

// ------------------------------------------------------------------- status

fn paint_status(ui: &mut Ui, c: &mut Chrome) {
    let r = c.regions.status;
    ui.bmp.fill_rect(r, ui.theme.status_bg);
    ui.divider_h(Rect::new(r.x, r.y, r.w, 1.0));
    let baseline = r.y + (r.h + ui.font.ascent(Metrics::SMALL_TEXT_SIZE) * 0.72) * 0.5;
    let mut x = 10.0;
    let small = Metrics::SMALL_TEXT_SIZE;
    let muted = ui.theme.text_muted;

    if let Some(doc) = current_doc(c) {
        if doc.is_ready() {
            let title = ui.font.ellipsize(&doc.title, small, 250.0).into_owned();
            ui.font.draw_text(ui.bmp, x, baseline, small, ui.theme.text, &title);
            x += ui.font.text_width(&title, small) + 16.0;
            let page = format!("Page {} of {}", doc.current_page() + 1, doc.page_count());
            ui.font.draw_text(ui.bmp, x, baseline, small, muted, &page);
            x += ui.font.text_width(&page, small) + 16.0;
            let zoom = format!("{:.0}%", doc.layout.zoom * 100.0);
            ui.font.draw_text(ui.bmp, x, baseline, small, muted, &zoom);
            x += ui.font.text_width(&zoom, small) + 16.0;
            if doc.session.rotation != 0 {
                let rot = format!("{}°", doc.session.rotation);
                ui.font.draw_text(ui.bmp, x, baseline, small, muted, &rot);
                x += ui.font.text_width(&rot, small) + 16.0;
            }
            let mode = match doc.session.reading_mode {
                ReadingMode::Continuous => "continuous",
                ReadingMode::Single => "single page",
            };
            ui.font.draw_text(ui.bmp, x, baseline, small, muted, mode);
            x += ui.font.text_width(mode, small) + 16.0;
            if doc.info().map(|i| i.encrypted).unwrap_or(false) {
                ui.font.draw_text(ui.bmp, x, baseline, small, ui.theme.warning, "encrypted");
            }
        } else if let Some(err) = &doc.error {
            let text = ui.font.ellipsize(err, small, r.w * 0.6).into_owned();
            ui.font.draw_text(ui.bmp, x, baseline, small, ui.theme.error, &text);
        }
    } else {
        ui.font.draw_text(ui.bmp, x, baseline, small, muted, "No document open");
    }

    let note = c.busy.clone().or_else(|| c.note.clone());
    if let Some(note) = note {
        let text = ui.font.ellipsize(&note, small, r.w * 0.35).into_owned();
        let w = ui.font.text_width(&text, small);
        ui.font.draw_text(
            ui.bmp,
            r.center().x - w * 0.5,
            baseline,
            small,
            ui.theme.text_muted,
            &text,
        );
    }

    let stats = format!(
        "{:.0} MB · render {:.0} ms · frame {:.1} ms",
        c.perf.cache_bytes as f32 / (1024.0 * 1024.0),
        c.perf.last_render_ms,
        c.perf.last_frame_ms
    );
    let w = ui.font.text_width(&stats, small);
    ui.font.draw_text(ui.bmp, r.right() - w - 10.0, baseline, small, muted, &stats);
}

fn paint_presentation_overlay(ui: &mut Ui, c: &mut Chrome) {
    let Some(doc) = current_doc(c) else { return };
    let text = format!("{} / {}", doc.current_page() + 1, doc.page_count());
    let w = ui.font.text_width(&text, Metrics::SMALL_TEXT_SIZE) + 22.0;
    let r = Rect::new(
        c.regions.window.center().x - w * 0.5,
        c.regions.window.bottom() - 46.0,
        w,
        24.0,
    );
    ui.bmp.fill_round_rect(r, 12.0, ui.theme.toast_bg.fade(0.35));
    ui.font.draw_text_centered(
        ui.bmp,
        Rect::new(r.x, r.y + 4.0, r.w, r.h),
        Metrics::SMALL_TEXT_SIZE,
        ui.theme.toast_text,
        &text,
    );
}

// ------------------------------------------------------------------- toasts

fn paint_toasts(ui: &mut Ui, c: &mut Chrome) {
    let mut y = c.regions.window.bottom() - Metrics::STATUS_H - 14.0;
    for toast in c.toasts.iter().rev() {
        let color = match toast.level {
            ToastLevel::Info => ui.theme.toast_bg,
            ToastLevel::Warn => ui.theme.warning,
            ToastLevel::Error => ui.theme.error,
        };
        let text = ui.font.ellipsize(&toast.text, Metrics::TEXT_SIZE, 380.0).into_owned();
        let w = ui.font.text_width(&text, Metrics::TEXT_SIZE) + 28.0;
        let r = Rect::new(c.regions.window.right() - w - 18.0, y - 34.0, w, 34.0);
        ui.bmp.drop_shadow(r, 8.0, 0.3, ui.theme.shadow);
        ui.bmp.fill_round_rect(r, 8.0, color);
        let fg = if toast.level == ToastLevel::Info {
            ui.theme.toast_text
        } else {
            Color::rgb(255, 255, 255)
        };
        ui.font.draw_text(
            ui.bmp,
            r.x + 14.0,
            r.y + (r.h + ui.font.ascent(Metrics::TEXT_SIZE) * 0.72) * 0.5,
            Metrics::TEXT_SIZE,
            fg,
            &text,
        );
        y -= 42.0;
    }
}

// ------------------------------------------------------------------ dialogs

/// Read-only view of app state that dialogs need.
struct DialogCtx<'a> {
    docs: &'a [DocView],
    active: usize,
    settings: &'a Settings,
    highlight_color: u32,
    db_error: &'a Option<String>,
}

impl<'a> DialogCtx<'a> {
    fn doc(&self) -> Option<&DocView> {
        self.docs.get(self.active)
    }
}

fn scrim(ui: &mut Ui) {
    let window = Rect::new(0.0, 0.0, ui.bmp.width as f32, ui.bmp.height as f32);
    ui.bmp.fill_rect(window, ui.theme.modal_scrim);
}

/// Draws the modal frame and returns the frame plus whether it was closed.
fn modal_frame(ui: &mut Ui, window: Rect, w: f32, h: f32, title: &str) -> (Rect, bool) {
    let w = w.min((window.w - 40.0).max(260.0));
    let h = h.min((window.h - 40.0).max(160.0));
    let r = Rect::new(window.x + (window.w - w) * 0.5, window.y + (window.h - h) * 0.5, w, h);
    ui.bmp.drop_shadow(r, 18.0, 0.4, ui.theme.shadow);
    ui.bmp.fill_round_rect(r, 10.0, ui.theme.modal_bg);
    ui.bmp.stroke_round_rect(r, 10.0, ui.theme.control_border, 1.0);
    ui.font.draw_text(
        ui.bmp,
        r.x + 18.0,
        r.y + 27.0,
        Metrics::TITLE_TEXT_SIZE,
        ui.theme.text,
        title,
    );
    let close = Rect::new(r.right() - 36.0, r.y + 10.0, 22.0, 22.0);
    let closed = widgets::icon_button(ui, id(&format!("dlg-close-{}", title)), close, Icon::Close, true, false);
    ui.divider_h(Rect::new(r.x + 1.0, r.y + 42.0, r.w - 2.0, 1.0));
    (r, closed)
}

fn paint_dialogs(ui: &mut Ui, d: &mut Dialogs, ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    scrim(ui);
    if d.open.is_some() {
        paint_open_dialog(ui, d, ctx, window, out);
    } else if d.goto.is_some() {
        paint_goto_dialog(ui, d, ctx, window, out);
    } else if d.resume.is_some() {
        paint_resume_dialog(ui, d, ctx, window, out);
    } else if d.password.is_some() {
        paint_password_dialog(ui, d, window, out);
    } else if d.settings {
        paint_settings_dialog(ui, d, ctx, window, out);
    } else if d.info {
        paint_info_dialog(ui, d, ctx, window, out);
    } else if d.shortcuts {
        paint_shortcuts_dialog(ui, d, window, out);
    }
    if d.error.is_some() {
        paint_error_dialog(ui, d, window, out);
    }
}

fn paint_open_dialog(ui: &mut Ui, d: &mut Dialogs, _ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 720.0, 540.0, "Open PDF");
    if closed {
        d.open = None;
        out.push(Action::CloseDialog);
        return;
    }
    let Some(dlg): Option<&mut FileDialog> = d.open.as_mut() else {
        return;
    };
    let inner = Rect::new(frame.x + 16.0, frame.y + 54.0, frame.w - 32.0, frame.h - 72.0);
    let path_rect = Rect::new(inner.x, inner.y, inner.w - 96.0, 30.0);
    let resp = widgets::text_field(ui, id("open-path"), path_rect, &mut dlg.path_field, "Path", false);
    if resp.submitted {
        let typed = std::path::PathBuf::from(dlg.path_field.text.trim());
        if typed.is_file() {
            out.push(Action::OpenPath(typed));
            return;
        }
        dlg.navigate_to(&typed);
    }
    let up = Rect::new(path_rect.right() + 8.0, path_rect.y, 34.0, 30.0);
    if widgets::button(ui, id("open-up"), up, "↑", true, false) {
        dlg.go_up();
    }
    let all = Rect::new(up.right() + 8.0, path_rect.y, 44.0, 30.0);
    if widgets::button(ui, id("open-all"), all, "All", true, dlg.show_all_files) {
        dlg.show_all_files = !dlg.show_all_files;
        dlg.refresh();
    }
    let list = Rect::new(inner.x, inner.y + 40.0, inner.w, (inner.h - 40.0 - 46.0).max(60.0));
    ui.bmp.fill_rect(list, ui.theme.window_bg);
    ui.bmp.stroke_round_rect(list, 6.0, ui.theme.control_border, 1.0);
    let row_h = 28.0;
    let content_h = dlg.entries.len() as f32 * row_h + 6.0;
    let mut offset = dlg.scroll;
    let area = Rect::new(list.x, list.y + 3.0, list.w, list.h - 6.0);
    let mut y = area.y - offset;
    let mut opened: Option<std::path::PathBuf> = None;
    let mut navigate: Option<std::path::PathBuf> = None;
    for (i, entry) in dlg.entries.iter().enumerate() {
        let r = Rect::new(area.x + 3.0, y, area.w - Metrics::SCROLLBAR_W - 8.0, row_h - 2.0);
        if r.bottom() > area.y && r.y < area.bottom() {
            let label = if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let subtitle = if entry.is_dir {
                None
            } else {
                Some(format!("{:.1} KB", entry.size as f64 / 1024.0))
            };
            let selected = dlg.selected == Some(i);
            if widgets::list_row(
                ui,
                id(&format!("open-entry-{}", i)),
                r,
                &label,
                subtitle.as_deref(),
                selected,
                18.0,
            ) {
                dlg.selected = Some(i);
                if entry.is_dir {
                    navigate = Some(entry.path.clone());
                } else if ui.input.double_click {
                    opened = Some(entry.path.clone());
                }
            }
            let icon_rect = Rect::new(r.x + 5.0, r.y + (r.h - 14.0) * 0.5, 14.0, 14.0);
            if entry.is_dir {
                icons::draw(ui.bmp, icon_rect, Icon::Folder, ui.theme.accent);
            }
        }
        y += row_h;
    }
    let changed = widgets::scrollbar(
        ui,
        id("open-scroll"),
        Rect::new(area.right() - Metrics::SCROLLBAR_W - 2.0, area.y, Metrics::SCROLLBAR_W, area.h),
        content_h,
        area.h,
        &mut offset,
    );
    if changed {
        dlg.scroll = offset;
    }
    if let Some(path) = navigate {
        dlg.navigate_to(&path);
    }
    if let Some(path) = opened {
        out.push(Action::OpenPath(path));
        return;
    }
    if let Some(err) = dlg.error.clone() {
        let text = ui.font.ellipsize(&err, Metrics::SMALL_TEXT_SIZE, inner.w).into_owned();
        ui.font.draw_text(
            ui.bmp,
            inner.x,
            list.bottom() + 16.0,
            Metrics::SMALL_TEXT_SIZE,
            ui.theme.error,
            &text,
        );
    }
    let buttons_y = frame.bottom() - 44.0;
    let cancel = Rect::new(frame.right() - 190.0, buttons_y, 84.0, 32.0);
    let open = Rect::new(frame.right() - 96.0, buttons_y, 84.0, 32.0);
    if widgets::button(ui, id("open-cancel"), cancel, "Cancel", true, false) {
        d.open = None;
        out.push(Action::CloseDialog);
        return;
    }
    let selected = dlg.selected_entry().map(|e| (e.path.clone(), e.is_dir));
    if widgets::button(ui, id("open-ok"), open, "Open", true, true) {
        if let Some((path, is_dir)) = selected {
            if is_dir {
                dlg.navigate_to(&path);
            } else {
                out.push(Action::OpenPath(path));
            }
        }
    }
}

fn paint_goto_dialog(ui: &mut Ui, d: &mut Dialogs, ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 340.0, 180.0, "Go to page");
    if closed {
        d.goto = None;
        out.push(Action::CloseDialog);
        return;
    }
    let Some(dlg): Option<&mut GotoDialog> = d.goto.as_mut() else {
        return;
    };
    let inner = Rect::new(frame.x + 18.0, frame.y + 58.0, frame.w - 36.0, frame.h - 76.0);
    let total = ctx.doc().map(|doc| doc.page_count()).unwrap_or(0);
    widgets::section_label(
        ui,
        Rect::new(inner.x, inner.y, inner.w, 16.0),
        &format!("Page number (1–{})", total.max(1)),
    );
    let field = Rect::new(inner.x, inner.y + 20.0, inner.w, 30.0);
    if ui.state.focus != Some(id("goto-field")) {
        ui.state.focus = Some(id("goto-field"));
    }
    let resp = widgets::text_field(ui, id("goto-field"), field, &mut dlg.field, "1", true);
    let parsed = dlg.field.text.trim().parse::<u32>().ok();
    let go = Rect::new(frame.right() - 104.0, frame.bottom() - 46.0, 86.0, 32.0);
    let clicked = widgets::button(ui, id("goto-ok"), go, "Go", parsed.is_some(), true);
    if (resp.submitted || clicked) && parsed.is_some() {
        out.push(Action::GoToPage(parsed.unwrap_or(1)));
        d.goto = None;
        return;
    }
    let cancel = Rect::new(go.x - 92.0, go.y, 84.0, 32.0);
    if widgets::button(ui, id("goto-cancel"), cancel, "Cancel", true, false) || resp.cancelled {
        d.goto = None;
        out.push(Action::CloseDialog);
    }
}

fn paint_resume_dialog(ui: &mut Ui, d: &mut Dialogs, ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 470.0, 238.0, "Resume reading?");
    if closed {
        d.resume = None;
        out.push(Action::CloseDialog);
        return;
    }
    let Some(dlg): Option<&mut ResumeDialog> = d.resume.as_mut() else {
        return;
    };
    let inner = Rect::new(frame.x + 20.0, frame.y + 62.0, frame.w - 40.0, frame.h - 80.0);
    let name = ctx
        .docs
        .get(dlg.doc_index)
        .map(|doc| doc.identity.display_name.clone())
        .unwrap_or_default();
    let msg = format!("You were last reading “{}” on page {} of {}.", name, dlg.page, dlg.total);
    let msg = ui.font.ellipsize(&msg, Metrics::TEXT_SIZE, inner.w).into_owned();
    ui.font.draw_text(ui.bmp, inner.x, inner.y + 12.0, Metrics::TEXT_SIZE, ui.theme.text, &msg);
    ui.font.draw_text(
        ui.bmp,
        inner.x,
        inner.y + 34.0,
        Metrics::TEXT_SIZE,
        ui.theme.text_muted,
        "Continue where you left off, or start from the first page?",
    );
    let remember = Rect::new(inner.x, inner.y + 52.0, inner.w, 26.0);
    dlg.remember = widgets::checkbox(ui, id("resume-remember"), remember, "Remember my choice", dlg.remember);
    let resume = Rect::new(inner.x, inner.y + 96.0, (inner.w - 12.0) * 0.5, 34.0);
    let restart = Rect::new(resume.right() + 12.0, resume.y, resume.w, 34.0);
    if widgets::button(ui, id("resume-yes"), resume, "Resume", true, true) {
        let remember = dlg.remember;
        d.resume = None;
        out.push(Action::ResumeAnswer { resume: true, remember });
    } else if widgets::button(ui, id("resume-no"), restart, "Start from the beginning", true, false) {
        let remember = dlg.remember;
        d.resume = None;
        out.push(Action::ResumeAnswer { resume: false, remember });
    }
}

fn paint_password_dialog(ui: &mut Ui, d: &mut Dialogs, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 420.0, 212.0, "Password required");
    if closed {
        d.password = None;
        out.push(Action::CloseDialog);
        return;
    }
    let Some(dlg): Option<&mut PasswordDialog> = d.password.as_mut() else {
        return;
    };
    let inner = Rect::new(frame.x + 20.0, frame.y + 60.0, frame.w - 40.0, frame.h - 78.0);
    let msg = format!("“{}” is protected. Enter its password.", dlg.document);
    let msg = ui.font.ellipsize(&msg, Metrics::TEXT_SIZE, inner.w).into_owned();
    ui.font.draw_text(ui.bmp, inner.x, inner.y + 14.0, Metrics::TEXT_SIZE, ui.theme.text, &msg);
    let field = Rect::new(inner.x, inner.y + 28.0, inner.w, 30.0);
    if ui.state.focus != Some(id("password-field")) {
        ui.state.focus = Some(id("password-field"));
    }
    let resp = widgets::text_field(ui, id("password-field"), field, &mut dlg.field, "Password", false);
    if let Some(err) = dlg.error.clone() {
        ui.font.draw_text(
            ui.bmp,
            inner.x,
            field.bottom() + 16.0,
            Metrics::SMALL_TEXT_SIZE,
            ui.theme.error,
            &err,
        );
    }
    let ok = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    let cancel = Rect::new(ok.x - 92.0, ok.y, 84.0, 32.0);
    if widgets::button(ui, id("password-cancel"), cancel, "Cancel", true, false) || resp.cancelled {
        d.password = None;
        out.push(Action::CloseDialog);
        return;
    }
    if widgets::button(ui, id("password-ok"), ok, "Unlock", true, true) || resp.submitted {
        let password = dlg.field.text.clone();
        d.password = None;
        out.push(Action::PasswordSubmit(password));
    }
}

fn paint_settings_dialog(ui: &mut Ui, d: &mut Dialogs, ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 560.0, 470.0, "Settings");
    if closed {
        d.settings = false;
        out.push(Action::SaveSettings);
        return;
    }
    let inner = Rect::new(frame.x + 20.0, frame.y + 54.0, frame.w - 40.0, frame.h - 70.0);
    let mut y = inner.y;
    widgets::section_label(ui, Rect::new(inner.x, y, inner.w, 16.0), "APPEARANCE");
    y += 20.0;
    if widgets::checkbox(ui, id("set-dark"), Rect::new(inner.x, y, inner.w, 26.0), "Dark theme (Ctrl+D)", ctx.settings.dark) {
        out.push(Action::ToggleTheme);
    }
    y += 34.0;
    if widgets::checkbox(
        ui,
        id("set-smooth"),
        Rect::new(inner.x, y, inner.w, 26.0),
        "Smooth scrolling",
        ctx.settings.smooth_scroll,
    ) {
        out.push(Action::SetSmoothScroll(!ctx.settings.smooth_scroll));
    }
    y += 34.0;
    widgets::section_label(ui, Rect::new(inner.x, y, inner.w, 16.0), "WHEN REOPENING A DOCUMENT");
    y += 22.0;
    for (i, policy) in [ResumePolicy::Ask, ResumePolicy::Always, ResumePolicy::Never].into_iter().enumerate() {
        let r = Rect::new(inner.x, y, inner.w, 26.0);
        let selected = ctx.settings.resume_policy == policy;
        if widgets::button(ui, id(&format!("set-policy-{}", i)), r, policy.label(), true, selected) {
            out.push(Action::SetResumePolicy(policy));
        }
        y += 30.0;
    }
    y += 8.0;
    widgets::section_label(ui, Rect::new(inner.x, y, inner.w, 16.0), "HIGHLIGHT COLOUR");
    y += 24.0;
    for (i, color) in [0xFFFF_E066u32, 0xFFA8_E6A1, 0xFF9A_D0F5, 0xFFF5_A8C8, 0xFFD9_B3F5]
        .into_iter()
        .enumerate()
    {
        let r = Rect::new(inner.x + 4.0 + i as f32 * 36.0, y, 26.0, 26.0);
        let hit = ui.interact(id(&format!("set-color-{}", i)), r, true);
        ui.bmp.fill_round_rect(r, 6.0, Color::from_argb(color));
        let selected = ctx.highlight_color == color;
        ui.bmp.stroke_round_rect(
            r,
            6.0,
            if selected { ui.theme.accent } else { ui.theme.control_border },
            if selected { 2.0 } else { 1.0 },
        );
        if hit {
            out.push(Action::SetHighlightColor(color));
        }
    }
    y += 38.0;
    match ctx.db_error {
        Some(err) => {
            let text = ui
                .font
                .ellipsize(&format!("Database: {}", err), Metrics::SMALL_TEXT_SIZE, inner.w)
                .into_owned();
            ui.font.draw_text(ui.bmp, inner.x, y + 12.0, Metrics::SMALL_TEXT_SIZE, ui.theme.warning, &text);
        }
        None => {
            ui.font.draw_text(
                ui.bmp,
                inner.x,
                y + 12.0,
                Metrics::SMALL_TEXT_SIZE,
                ui.theme.text_muted,
                "Reading positions, highlights and preferences are stored locally in SQLite.",
            );
        }
    }
    let keys = Rect::new(frame.x + 20.0, frame.bottom() - 44.0, 130.0, 32.0);
    if widgets::button(ui, id("set-keys"), keys, "Shortcuts…", true, false) {
        out.push(Action::ShowShortcuts(true));
    }
    let done = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    if widgets::button(ui, id("set-close"), done, "Done", true, true) {
        d.settings = false;
        out.push(Action::SaveSettings);
    }
}

fn paint_info_dialog(ui: &mut Ui, d: &mut Dialogs, ctx: &DialogCtx, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 560.0, 420.0, "Document information");
    if closed {
        d.info = false;
        out.push(Action::CloseDialog);
        return;
    }
    let inner = Rect::new(frame.x + 20.0, frame.y + 56.0, frame.w - 40.0, frame.h - 72.0);
    let mut rows: Vec<(String, String)> = Vec::new();
    if let Some(doc) = ctx.doc() {
        rows.push(("File".into(), doc.identity.path.to_string_lossy().into_owned()));
        rows.push(("Title".into(), doc.title.clone()));
        if let Some(info) = doc.info() {
            if let Some(author) = &info.author {
                rows.push(("Author".into(), author.clone()));
            }
            if let Some(subject) = &info.subject {
                rows.push(("Subject".into(), subject.clone()));
            }
            if let Some(creator) = &info.creator {
                rows.push(("Producer".into(), creator.clone()));
            }
            rows.push(("Pages".into(), info.page_count.to_string()));
            rows.push(("PDF version".into(), format!("1.{}", info.file_version.max(0))));
            if info.encrypted {
                rows.push((
                    "Security".into(),
                    format!("Encrypted (revision {})", info.security_revision),
                ));
            }
        }
        rows.push(("Fingerprint".into(), doc.identity.fingerprint.clone()));
        rows.push((
            "Size".into(),
            format!("{:.2} MB", doc.identity.size as f64 / (1024.0 * 1024.0)),
        ));
        if let Some(modified) = doc.identity.modified {
            rows.push(("Modified".into(), format_unix_seconds(modified)));
        }
        rows.push((
            "Reading position".into(),
            format!(
                "page {} · {}% · {}",
                doc.session.last_page,
                (doc.session.zoom * 100.0).round() as i32,
                match doc.session.zoom_mode {
                    ZoomMode::FitPage => "fit page",
                    ZoomMode::FitWidth => "fit width",
                    ZoomMode::Actual => "actual size",
                    ZoomMode::Custom => "custom zoom",
                }
            ),
        ));
        rows.push(("Highlights".into(), doc.highlights.len().to_string()));
        rows.push((
            "Memory".into(),
            format!("{:.1} MB", doc.memory_bytes() as f64 / (1024.0 * 1024.0)),
        ));
    } else {
        rows.push(("Document".into(), "No document open".into()));
    }
    let mut y = inner.y;
    for (key, value) in rows {
        ui.font.draw_text(ui.bmp, inner.x, y + 12.0, Metrics::SMALL_TEXT_SIZE, ui.theme.text_muted, &key);
        let text = ui.font.ellipsize(&value, Metrics::TEXT_SIZE, inner.w - 140.0).into_owned();
        ui.font.draw_text(ui.bmp, inner.x + 140.0, y + 12.0, Metrics::TEXT_SIZE, ui.theme.text, &text);
        y += 24.0;
        if y > inner.bottom() {
            break;
        }
    }
    let close = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    if widgets::button(ui, id("info-close"), close, "Close", true, true) {
        d.info = false;
        out.push(Action::CloseDialog);
    }
}

/// Formats a Unix timestamp (seconds) as an ISO-8601 UTC stamp.
fn format_unix_seconds(secs: u64) -> String {
    let secs = secs as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, day) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        y,
        m,
        day,
        rem / 3600,
        (rem % 3600) / 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}

fn paint_shortcuts_dialog(ui: &mut Ui, d: &mut Dialogs, window: Rect, out: &mut Vec<Action>) {
    let (frame, closed) = modal_frame(ui, window, 640.0, 480.0, "Keyboard shortcuts");
    if closed {
        d.shortcuts = false;
        out.push(Action::CloseDialog);
        return;
    }
    let inner = Rect::new(frame.x + 20.0, frame.y + 56.0, frame.w - 40.0, frame.h - 72.0);
    let shortcuts: &[(&str, &str)] = &[
        ("Ctrl+O", "Open a PDF"),
        ("Ctrl+W", "Close the current document"),
        ("Ctrl+Q", "Quit"),
        ("← → ↑ ↓ / PgUp PgDn", "Previous or next page"),
        ("Home / End", "First or last page"),
        ("Ctrl+G", "Go to page"),
        ("Ctrl++ / Ctrl+-", "Zoom in or out"),
        ("Ctrl+0 / Ctrl+1 / Ctrl+2", "Fit page / fit width / actual size"),
        ("Ctrl+wheel", "Zoom at the pointer"),
        ("Ctrl+R / Ctrl+Shift+R", "Rotate clockwise / counter-clockwise"),
        ("Ctrl+Alt+R", "Rotate 180°"),
        ("Ctrl+M", "Toggle single-page mode"),
        ("Ctrl+F / F3 / Shift+F3", "Search, next match, previous match"),
        ("Ctrl+C", "Copy selection"),
        ("Ctrl+H", "Highlight selection"),
        ("Ctrl+P", "Print"),
        ("Ctrl+E", "Export a rotated copy"),
        ("Ctrl+D", "Toggle dark theme"),
        ("F9", "Toggle sidebar"),
        ("F5", "Presentation mode"),
        ("F11", "Full screen"),
        ("Esc", "Close dialog or leave full screen"),
    ];
    let col_w = inner.w * 0.5;
    let row_h = 20.0;
    let per_col = ((inner.h - 40.0) / row_h) as usize;
    for (i, (key, what)) in shortcuts.iter().enumerate() {
        let col = i / per_col.max(1);
        let row = i % per_col.max(1);
        let x = inner.x + col as f32 * col_w;
        let y = inner.y + row as f32 * row_h;
        ui.font.draw_text(ui.bmp, x, y + 13.0, Metrics::SMALL_TEXT_SIZE, ui.theme.accent, key);
        let text = ui.font.ellipsize(what, Metrics::SMALL_TEXT_SIZE, col_w - 160.0).into_owned();
        ui.font.draw_text(ui.bmp, x + 162.0, y + 13.0, Metrics::SMALL_TEXT_SIZE, ui.theme.text, &text);
    }
    let close = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    if widgets::button(ui, id("keys-close"), close, "Close", true, true) {
        d.shortcuts = false;
        out.push(Action::CloseDialog);
    }
}

fn paint_error_dialog(ui: &mut Ui, d: &mut Dialogs, window: Rect, out: &mut Vec<Action>) {
    let Some(error): Option<ErrorDialog> = d.error.clone() else {
        return;
    };
    let (frame, closed) = modal_frame(ui, window, 500.0, 230.0, &error.title);
    if closed {
        d.error = None;
        out.push(Action::DismissError);
        return;
    }
    let inner = Rect::new(frame.x + 20.0, frame.y + 62.0, frame.w - 40.0, frame.h - 80.0);
    let mut y = inner.y;
    for line in wrap(&error.message, 64) {
        let text = ui.font.ellipsize(&line, Metrics::TEXT_SIZE, inner.w).into_owned();
        ui.font.draw_text(ui.bmp, inner.x, y + 12.0, Metrics::TEXT_SIZE, ui.theme.text, &text);
        y += 20.0;
        if y > inner.bottom() - 44.0 {
            break;
        }
    }
    let ok = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    if widgets::button(ui, id("error-ok"), ok, "OK", true, true) {
        d.error = None;
        out.push(Action::DismissError);
    }
}

fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.len() + word.len() + 1 > max && !line.is_empty() {
            lines.push(line.clone());
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_inside_preserves_aspect_ratio() {
        let area = Rect::new(0.0, 0.0, 100.0, 100.0);
        let r = fit_inside(area, 200.0, 100.0);
        assert_eq!((r.w.round(), r.h.round()), (100.0, 50.0));
        assert_eq!(r.y, 25.0);
    }

    #[test]
    fn wrapper_splits_long_text() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 12);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.len() <= 12));
        assert_eq!(wrap("", 10).len(), 1);
    }

    #[test]
    fn flatten_outline_respects_collapsed_state() {
        let items = vec![crate::pdf::OutlineItem {
            title: "Chapter".into(),
            page: Some(0),
            uri: None,
            children: vec![crate::pdf::OutlineItem {
                title: "Section".into(),
                page: Some(1),
                uri: None,
                children: vec![],
            }],
        }];
        let empty = std::collections::HashSet::new();
        assert_eq!(flatten_outline(&items, 0.0, &empty).len(), 1);
        let mut expanded = std::collections::HashSet::new();
        expanded.insert("Chapter".to_string());
        assert_eq!(flatten_outline(&items, 0.0, &expanded).len(), 2);
    }

    #[test]
    fn civil_dates_are_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }

    #[test]
    fn snippets_are_trimmed_around_the_match() {
        let text = crate::pdf::PageText {
            text: "alpha beta gamma delta epsilon zeta".into(),
            chars: Vec::new(),
        };
        let s = snippet_for(&text, 11, 5);
        assert!(s.contains("gamma"), "{s}");
    }
}
