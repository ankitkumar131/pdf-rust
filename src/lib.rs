//! `pdf-rust` — a fast, lightweight, native PDF reader.
//!
//! The crate is split into an application library (this crate) and a thin
//! platform shell (see `platform`), so all behaviour can be tested headlessly
//! without a display server.
//!
//! Module map:
//! - [`app`]      — application state, view layout, persistence wiring.
//! - [`doc`]      — document identity (canonical path + content fingerprint).
//! - [`platform`] — X11/Win32 shells, printing and OS integration.
//! - [`geom`]     — geometry primitives.
//! - [`ui`]       — software rasterizer, font engine, widgets, reader chrome.
//! - [`pdf`]      — PDFium bindings and the rendering worker.
//! - [`db`]       — SQLite persistence (documents, sessions, settings).
//! - [`util`]     — small helpers (PNG dumping, paths, logging).

pub mod app;
pub mod db;
pub mod doc;
pub mod geom;
pub mod pdf;
pub mod platform;
pub mod ui;
pub mod util;

/// Product name used in window titles and user-facing strings.
pub const APP_NAME: &str = "pdf-rust";
/// Version taken from Cargo metadata.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
