//! `pdf-rust` — a fast, lightweight, native PDF reader.
//!
//! The crate is split into an application library (this crate) and a thin
//! platform shell (see `platform`), so all behaviour can be tested headlessly
//! without a display server.
//!
//! Module map:
//! - [`geom`]     — geometry primitives.
//! - [`ui`]       — software rasterizer, font engine, widgets, reader chrome.
//! - [`pdf`]      — PDFium bindings and the rendering worker.
//! - [`db`]       — SQLite persistence (documents, sessions, settings).
//! - [`util`]     — small helpers (PNG dumping, paths, logging).

pub mod geom;
pub mod pdf;
pub mod ui;
pub mod util;

/// Product name used in window titles and user-facing strings.
pub const APP_NAME: &str = "pdf-rust";
/// Version taken from Cargo metadata.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
