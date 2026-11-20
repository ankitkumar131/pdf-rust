//! PDF document handling: PDFium bindings, the background rendering worker and
//! document identity helpers.

pub mod engine;
pub mod ffi;

pub use engine::{
    rotated, CharBox, Command, DocId, DocInfo, Engine, Event, LinkRect, LinkTarget, OutlineItem,
    PageText, RenderKey, SearchHit,
};
