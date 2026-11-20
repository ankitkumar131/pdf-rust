//! Persistent storage: documents, per-document reading sessions, settings and
//! highlights, backed by SQLite.
//!
//! Design notes
//! ------------
//! * The database stores **metadata only** — never document contents.
//! * Schema changes go through forward-only migrations keyed by
//!   `PRAGMA user_version`, applied inside a transaction.
//! * Every write is either a single statement (implicitly atomic) or wrapped in
//!   [`sql::transaction`], so a crash can never leave a half-written session.
//! * `journal_mode = WAL` + `synchronous = NORMAL` gives crash safety without
//!   paying an fsync per keystroke; progress writes are throttled by the caller.

pub mod sql;

use std::path::{Path, PathBuf};

use crate::geom::Rect;

/// How pages are laid out in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadingMode {
    #[default]
    Continuous,
    Single,
}

impl ReadingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadingMode::Continuous => "continuous",
            ReadingMode::Single => "single",
        }
    }

    pub fn parse(s: &str) -> ReadingMode {
        match s {
            "single" => ReadingMode::Single,
            _ => ReadingMode::Continuous,
        }
    }
}

/// How the zoom factor is derived from the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    FitPage,
    FitWidth,
    Actual,
    Custom,
}

impl ZoomMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ZoomMode::FitPage => "fit-page",
            ZoomMode::FitWidth => "fit-width",
            ZoomMode::Actual => "actual",
            ZoomMode::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> ZoomMode {
        match s {
            "fit-width" => ZoomMode::FitWidth,
            "actual" => ZoomMode::Actual,
            "custom" => ZoomMode::Custom,
            _ => ZoomMode::FitPage,
        }
    }
}

/// Per-document view state that must survive restarts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionRow {
    /// 1-based page number as shown to the user.
    pub last_page: u32,
    /// Scroll position inside that page, 0.0 = top, 1.0 = bottom.
    pub scroll_frac: f32,
    /// Zoom factor; ignored when `zoom_mode != Custom`.
    pub zoom: f32,
    pub zoom_mode: ZoomMode,
    /// User rotation in degrees (0/90/180/270), on top of the page's own.
    pub rotation: u16,
    pub reading_mode: ReadingMode,
}

impl Default for SessionRow {
    fn default() -> Self {
        SessionRow {
            last_page: 1,
            scroll_frac: 0.0,
            zoom: 1.0,
            zoom_mode: ZoomMode::FitPage,
            rotation: 0,
            reading_mode: ReadingMode::Continuous,
        }
    }
}

impl SessionRow {
    pub fn clamp_values(&mut self) {
        if self.last_page == 0 {
            self.last_page = 1;
        }
        self.scroll_frac = self.scroll_frac.clamp(0.0, 1.0);
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            self.zoom = 1.0;
        }
        self.zoom = self.zoom.clamp(0.05, 40.0);
        self.rotation = match self.rotation % 360 {
            90 => 90,
            180 => 180,
            270 => 270,
            _ => 0,
        };
    }
}

/// A document known to the reader.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRow {
    pub id: i64,
    pub fingerprint: String,
    pub path: String,
    pub title: Option<String>,
    pub page_count: u32,
    pub session: SessionRow,
    /// Unix timestamp (seconds) of the last time the document was opened.
    pub last_opened: i64,
}

/// A highlight annotation. Stored in the database only — the original PDF is
/// never modified (see the security section of the README).
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightRow {
    pub id: i64,
    pub page: u32,
    /// Rectangles in PDF points (y up) that make up the highlight.
    pub rects: Vec<Rect>,
    pub color: u32,
    pub note: Option<String>,
    pub created_at: i64,
}

/// Forward-only schema migrations. Index + 1 == resulting `user_version`.
pub const MIGRATIONS: &[&str] = &[
    // v1 — documents, sessions, settings, highlights.
    r#"
    CREATE TABLE documents (
        id           INTEGER PRIMARY KEY,
        fingerprint  TEXT    NOT NULL UNIQUE,
        path         TEXT    NOT NULL,
        title        TEXT,
        page_count   INTEGER NOT NULL DEFAULT 0,
        created_at   INTEGER NOT NULL,
        last_opened  INTEGER NOT NULL
    );
    CREATE INDEX documents_last_opened ON documents(last_opened DESC);
    CREATE TABLE sessions (
        document_id  INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
        last_page    INTEGER NOT NULL DEFAULT 1,
        scroll_frac  REAL    NOT NULL DEFAULT 0,
        zoom         REAL    NOT NULL DEFAULT 1,
        zoom_mode    TEXT    NOT NULL DEFAULT 'fit-page',
        rotation     INTEGER NOT NULL DEFAULT 0,
        reading_mode TEXT    NOT NULL DEFAULT 'continuous',
        updated_at   INTEGER NOT NULL
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE highlights (
        id          INTEGER PRIMARY KEY,
        document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        page        INTEGER NOT NULL,
        rects       TEXT    NOT NULL,
        color       INTEGER NOT NULL DEFAULT 4286866854,
        note        TEXT,
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX highlights_document ON highlights(document_id, page);
    "#,
    // v2 — the documents that were open, in tab order, plus which one was active.
    r#"
    CREATE TABLE IF NOT EXISTS open_documents (
        document_id INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
        position    INTEGER NOT NULL,
        active      INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS open_documents_position ON open_documents(position);
    "#,
];

pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

/// The reader's local database.
pub struct Database {
    conn: sql::Sqlite,
    path: PathBuf,
}

impl Database {
    /// Opens (and migrates) the database file.
    pub fn open(path: &Path) -> Result<Database, String> {
        let conn = sql::Sqlite::open(path)?;
        let db = Database {
            conn,
            path: path.to_path_buf(),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Opens a throw-away database (tests, and the fallback when the user's data
    /// directory is not writable).
    pub fn open_in_memory() -> Result<Database, String> {
        let dir = std::env::temp_dir().join(format!(
            "pdf-rust-mem-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut db = Database::open(&dir.join("scratch.db"))?;
        db.path = PathBuf::from(":memory:");
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i32, String> {
        let mut stmt = self.conn.prepare("PRAGMA user_version")?;
        if stmt.step()? {
            Ok(stmt.column_i64(0) as i32)
        } else {
            Ok(0)
        }
    }

    /// Applies any pending migrations inside a transaction.
    pub fn migrate(&self) -> Result<(), String> {
        let current = self.schema_version()?;
        if current > SCHEMA_VERSION {
            return Err(format!(
                "the database was created by a newer version of pdf-rust (schema {}, this build understands {})",
                current, SCHEMA_VERSION
            ));
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let version = index as i32 + 1;
            sql::transaction(&self.conn, |c| {
                c.exec(migration)?;
                c.exec(&format!("PRAGMA user_version = {}", version))?;
                Ok(())
            })
            .map_err(|e| format!("migration to schema v{} failed: {}", version, e))?;
        }
        Ok(())
    }

    /// Inserts a document or refreshes its path/title/page count, returning its
    /// row id. `now` is a Unix timestamp.
    pub fn upsert_document(
        &self,
        fingerprint: &str,
        path: &str,
        title: Option<&str>,
        page_count: u32,
        now: i64,
    ) -> Result<i64, String> {
        sql::transaction(&self.conn, |c| {
            if let Some(id) = lookup_id(c, fingerprint)? {
                let mut stmt = c.prepare(
                    "UPDATE documents SET path = ?1, title = ?2, page_count = ?3, last_opened = ?4 WHERE id = ?5",
                )?;
                stmt.bind_text(1, path);
                match title {
                    Some(t) => stmt.bind_text(2, t),
                    None => stmt.bind_null(2),
                }
                stmt.bind_i64(3, page_count as i64);
                stmt.bind_i64(4, now);
                stmt.bind_i64(5, id);
                stmt.step()?;
                // Ensure a session row exists for every document.
                c.exec(&format!(
                    "INSERT OR IGNORE INTO sessions (document_id, updated_at) VALUES ({}, {})",
                    id, now
                ))?;
                Ok(id)
            } else {
                let mut stmt = c.prepare(
                    "INSERT INTO documents (fingerprint, path, title, page_count, created_at, last_opened)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                )?;
                stmt.bind_text(1, fingerprint);
                stmt.bind_text(2, path);
                match title {
                    Some(t) => stmt.bind_text(3, t),
                    None => stmt.bind_null(3),
                }
                stmt.bind_i64(4, page_count as i64);
                stmt.bind_i64(5, now);
                stmt.step()?;
                let id = c.last_insert_rowid();
                c.exec(&format!(
                    "INSERT OR IGNORE INTO sessions (document_id, updated_at) VALUES ({}, {})",
                    id, now
                ))?;
                Ok(id)
            }
        })
    }

    /// Looks a document up by fingerprint (survives moves and renames).
    pub fn find_by_fingerprint(&self, fingerprint: &str) -> Result<Option<DocumentRow>, String> {
        let mut stmt = self.conn.prepare(&document_select("WHERE d.fingerprint = ?1"))?;
        stmt.bind_text(1, fingerprint);
        self.read_document_row(stmt)
    }

    /// Looks a document up by its stored path.
    pub fn find_by_path(&self, path: &str) -> Result<Option<DocumentRow>, String> {
        let mut stmt = self.conn.prepare(&document_select("WHERE d.path = ?1"))?;
        stmt.bind_text(1, path);
        self.read_document_row(stmt)
    }

    pub fn document_by_id(&self, id: i64) -> Result<Option<DocumentRow>, String> {
        let mut stmt = self.conn.prepare(&document_select("WHERE d.id = ?1"))?;
        stmt.bind_i64(1, id);
        self.read_document_row(stmt)
    }

    fn read_document_row(&self, mut stmt: sql::Stmt<'_>) -> Result<Option<DocumentRow>, String> {
        if !stmt.step()? {
            return Ok(None);
        }
        Ok(Some(DocumentRow {
            id: stmt.column_i64(0),
            fingerprint: stmt.column_text(1),
            path: stmt.column_text(2),
            title: if stmt.is_null(3) {
                None
            } else {
                Some(stmt.column_text(3))
            },
            page_count: stmt.column_i64(4).max(0) as u32,
            session: SessionRow {
                last_page: stmt.column_i64(5).max(1) as u32,
                scroll_frac: stmt.column_f64(6) as f32,
                zoom: stmt.column_f64(7) as f32,
                zoom_mode: ZoomMode::parse(&stmt.column_text(8)),
                rotation: (stmt.column_i64(9).rem_euclid(360)) as u16,
                reading_mode: ReadingMode::parse(&stmt.column_text(10)),
            },
            last_opened: stmt.column_i64(11),
        }))
    }

    /// Documents ordered by most recently opened.
    pub fn recent(&self, limit: u32) -> Result<Vec<DocumentRow>, String> {
        let mut stmt = self
            .conn
            .prepare(&document_select("ORDER BY d.last_opened DESC LIMIT ?1"))?;
        stmt.bind_i64(1, limit as i64);
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(DocumentRow {
                id: stmt.column_i64(0),
                fingerprint: stmt.column_text(1),
                path: stmt.column_text(2),
                title: if stmt.is_null(3) {
                    None
                } else {
                    Some(stmt.column_text(3))
                },
                page_count: stmt.column_i64(4).max(0) as u32,
                session: SessionRow {
                    last_page: stmt.column_i64(5).max(1) as u32,
                    scroll_frac: stmt.column_f64(6) as f32,
                    zoom: stmt.column_f64(7) as f32,
                    zoom_mode: ZoomMode::parse(&stmt.column_text(8)),
                    rotation: stmt.column_i64(9).rem_euclid(360) as u16,
                    reading_mode: ReadingMode::parse(&stmt.column_text(10)),
                },
                last_opened: stmt.column_i64(11),
            });
        }
        Ok(out)
    }

    pub fn load_session(&self, document_id: i64) -> Result<Option<SessionRow>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT last_page, scroll_frac, zoom, zoom_mode, rotation, reading_mode
             FROM sessions WHERE document_id = ?1",
        )?;
        stmt.bind_i64(1, document_id);
        if !stmt.step()? {
            return Ok(None);
        }
        Ok(Some(SessionRow {
            last_page: stmt.column_i64(0).max(1) as u32,
            scroll_frac: stmt.column_f64(1) as f32,
            zoom: stmt.column_f64(2) as f32,
            zoom_mode: ZoomMode::parse(&stmt.column_text(3)),
            rotation: stmt.column_i64(4).rem_euclid(360) as u16,
            reading_mode: ReadingMode::parse(&stmt.column_text(5)),
        }))
    }

    /// Writes the reading position for a document (atomic upsert).
    pub fn save_session(
        &self,
        document_id: i64,
        session: &SessionRow,
        now: i64,
    ) -> Result<(), String> {
        let mut session = *session;
        session.clamp_values();
        let mut stmt = self.conn.prepare(
            "INSERT INTO sessions (document_id, last_page, scroll_frac, zoom, zoom_mode, rotation, reading_mode, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(document_id) DO UPDATE SET
                last_page = excluded.last_page,
                scroll_frac = excluded.scroll_frac,
                zoom = excluded.zoom,
                zoom_mode = excluded.zoom_mode,
                rotation = excluded.rotation,
                reading_mode = excluded.reading_mode,
                updated_at = excluded.updated_at",
        )?;
        stmt.bind_i64(1, document_id);
        stmt.bind_i64(2, session.last_page as i64);
        stmt.bind_f64(3, session.scroll_frac as f64);
        stmt.bind_f64(4, session.zoom as f64);
        stmt.bind_text(5, session.zoom_mode.as_str());
        stmt.bind_i64(6, session.rotation as i64);
        stmt.bind_text(7, session.reading_mode.as_str());
        stmt.bind_i64(8, now);
        stmt.step()?;
        Ok(())
    }

    /// Marks a document as opened now (drives the recent list).
    pub fn touch(&self, document_id: i64, now: i64) -> Result<(), String> {
        let mut stmt = self.conn.prepare("UPDATE documents SET last_opened = ?1 WHERE id = ?2")?;
        stmt.bind_i64(1, now);
        stmt.bind_i64(2, document_id);
        stmt.step()?;
        Ok(())
    }

    /// Removes a document (and, via cascade, its session and highlights).
    pub fn forget_document(&self, document_id: i64) -> Result<(), String> {
        let mut stmt = self.conn.prepare("DELETE FROM documents WHERE id = ?1")?;
        stmt.bind_i64(1, document_id);
        stmt.step()?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?;
        stmt.bind_text(1, key);
        stmt.bind_text(2, value);
        stmt.step()?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        stmt.bind_text(1, key);
        if stmt.step()? {
            Ok(Some(stmt.column_text(0)))
        } else {
            Ok(None)
        }
    }

    pub fn setting_f64(&self, key: &str, default: f64) -> f64 {
        self.get_setting(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.get_setting(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.get_setting(key)
            .ok()
            .flatten()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(default)
    }

    /// Replaces the saved list of open documents.
    ///
    /// `open` is `(document_id, is_active)` in tab order; it is written in a
    /// single transaction so a crash can never record a half-updated session.
    pub fn save_open_documents(&self, open: &[(i64, bool)]) -> Result<(), String> {
        sql::transaction(&self.conn, |c| {
            c.exec("DELETE FROM open_documents")?;
            for (position, (id, active)) in open.iter().enumerate() {
                let mut stmt = c.prepare(
                    "INSERT OR REPLACE INTO open_documents (document_id, position, active)
                     VALUES (?1, ?2, ?3)",
                )?;
                stmt.bind_i64(1, *id);
                stmt.bind_i64(2, position as i64);
                stmt.bind_i64(3, i64::from(*active));
                stmt.step()?;
            }
            Ok(())
        })
    }

    /// The saved open documents in tab order, with the active tab's position.
    pub fn open_documents(&self) -> Result<Vec<(i64, bool)>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT document_id, active FROM open_documents ORDER BY position ASC, document_id ASC",
        )?;
        let mut out = Vec::new();
        while stmt.step()? {
            out.push((stmt.column_i64(0), stmt.column_i64(1) != 0));
        }
        Ok(out)
    }

    pub fn add_highlight(
        &self,
        document_id: i64,
        page: u32,
        rects: &[Rect],
        color: u32,
        note: Option<&str>,
        now: i64,
    ) -> Result<i64, String> {
        let encoded = encode_rects(rects);
        let mut stmt = self.conn.prepare(
            "INSERT INTO highlights (document_id, page, rects, color, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        stmt.bind_i64(1, document_id);
        stmt.bind_i64(2, page as i64);
        stmt.bind_text(3, &encoded);
        stmt.bind_i64(4, color as i64);
        match note {
            Some(n) => stmt.bind_text(5, n),
            None => stmt.bind_null(5),
        }
        stmt.bind_i64(6, now);
        stmt.step()?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn highlights(&self, document_id: i64) -> Result<Vec<HighlightRow>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT id, page, rects, color, note, created_at FROM highlights
             WHERE document_id = ?1 ORDER BY page, id",
        )?;
        stmt.bind_i64(1, document_id);
        let mut out = Vec::new();
        while stmt.step()? {
            out.push(HighlightRow {
                id: stmt.column_i64(0),
                page: stmt.column_i64(1).max(0) as u32,
                rects: decode_rects(&stmt.column_text(2)),
                color: stmt.column_i64(3) as u32,
                note: if stmt.is_null(4) {
                    None
                } else {
                    Some(stmt.column_text(4))
                },
                created_at: stmt.column_i64(5),
            });
        }
        Ok(out)
    }

    pub fn delete_highlight(&self, id: i64) -> Result<(), String> {
        let mut stmt = self.conn.prepare("DELETE FROM highlights WHERE id = ?1")?;
        stmt.bind_i64(1, id);
        stmt.step()?;
        Ok(())
    }
}

/// SQL shared by the document lookups.
fn document_select(tail: &str) -> String {
    format!(
        "SELECT d.id, d.fingerprint, d.path, d.title, d.page_count,
                COALESCE(s.last_page, 1), COALESCE(s.scroll_frac, 0),
                COALESCE(s.zoom, 1), COALESCE(s.zoom_mode, 'fit-page'),
                COALESCE(s.rotation, 0), COALESCE(s.reading_mode, 'continuous'),
                d.last_opened
         FROM documents d LEFT JOIN sessions s ON s.document_id = d.id
         {}",
        tail
    )
}

fn lookup_id(conn: &sql::Sqlite, fingerprint: &str) -> Result<Option<i64>, String> {
    let mut stmt = conn.prepare("SELECT id FROM documents WHERE fingerprint = ?1")?;
    stmt.bind_text(1, fingerprint);
    if stmt.step()? {
        Ok(Some(stmt.column_i64(0)))
    } else {
        Ok(None)
    }
}

pub fn encode_rects(rects: &[Rect]) -> String {
    let mut out = String::with_capacity(rects.len() * 24);
    for (i, r) in rects.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        out.push_str(&format!("{:.2},{:.2},{:.2},{:.2}", r.x, r.y, r.w, r.h));
    }
    out
}

pub fn decode_rects(encoded: &str) -> Vec<Rect> {
    let mut out = Vec::new();
    for part in encoded.split(';') {
        let nums: Vec<f32> = part
            .split(',')
            .filter_map(|n| n.trim().parse::<f32>().ok())
            .collect();
        if nums.len() == 4 {
            out.push(Rect::new(nums[0], nums[1], nums[2], nums[3]));
        }
    }
    out
}

/// Default location of the database.
///
/// * Windows: `%LOCALAPPDATA%\pdf-rust\library.db`
/// * Linux: `$XDG_DATA_HOME/pdf-rust/library.db` or `~/.local/share/pdf-rust/…`
pub fn default_database_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(base).join("pdf-rust").join("library.db"))
    } else {
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local").join("share")))?;
        Some(base.join("pdf-rust").join("library.db"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::open_in_memory().expect("sqlite should be available for tests")
    }

    #[test]
    fn migration_sets_version_and_is_idempotent() {
        let db = db();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn document_upsert_is_idempotent_by_fingerprint() {
        let db = db();
        let id1 = db
            .upsert_document("fp-1", "/docs/a.pdf", Some("A"), 10, 100)
            .unwrap();
        let id2 = db
            .upsert_document("fp-1", "/docs/renamed.pdf", Some("A2"), 10, 200)
            .unwrap();
        assert_eq!(id1, id2, "same fingerprint must reuse the row");
        let row = db.find_by_fingerprint("fp-1").unwrap().unwrap();
        assert_eq!(row.path, "/docs/renamed.pdf");
        assert_eq!(row.title.as_deref(), Some("A2"));
        assert_eq!(row.last_opened, 200);
    }

    #[test]
    fn open_document_list_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let a = db.upsert_document("fp-a", "/a.pdf", None, 1, 10).unwrap();
        let b = db.upsert_document("fp-b", "/b.pdf", None, 2, 11).unwrap();
        let c = db.upsert_document("fp-c", "/c.pdf", None, 3, 12).unwrap();
        assert!(db.open_documents().unwrap().is_empty());
        db.save_open_documents(&[(c, false), (a, false), (b, true)])
            .unwrap();
        assert_eq!(
            db.open_documents().unwrap(),
            vec![(c, false), (a, false), (b, true)],
            "tab order and the active tab survive"
        );
        // Saving again replaces the list instead of appending to it.
        db.save_open_documents(&[(a, true)]).unwrap();
        assert_eq!(db.open_documents().unwrap(), vec![(a, true)]);
        // Removing a document also forgets it in the session list.
        db.forget_document(b).unwrap();
        assert_eq!(db.open_documents().unwrap(), vec![(a, true)]);
        db.save_open_documents(&[]).unwrap();
        assert!(db.open_documents().unwrap().is_empty());
    }

    #[test]
    fn session_round_trip_and_clamping() {
        let db = db();
        let id = db.upsert_document("fp-2", "/docs/b.pdf", None, 20, 10).unwrap();
        let mut session = SessionRow {
            last_page: 10,
            scroll_frac: 0.42,
            zoom: 1.75,
            zoom_mode: ZoomMode::Custom,
            rotation: 90,
            reading_mode: ReadingMode::Single,
        };
        db.save_session(id, &session, 20).unwrap();
        let loaded = db.load_session(id).unwrap().unwrap();
        assert_eq!(loaded.last_page, 10);
        assert!((loaded.scroll_frac - 0.42).abs() < 0.001);
        assert!((loaded.zoom - 1.75).abs() < 0.001);
        assert_eq!(loaded.zoom_mode, ZoomMode::Custom);
        assert_eq!(loaded.rotation, 90);
        assert_eq!(loaded.reading_mode, ReadingMode::Single);

        // Out-of-range values are clamped on write.
        session.last_page = 0;
        session.rotation = 450;
        session.zoom = -3.0;
        session.scroll_frac = 4.0;
        db.save_session(id, &session, 30).unwrap();
        let loaded = db.load_session(id).unwrap().unwrap();
        assert_eq!(loaded.last_page, 1);
        assert_eq!(loaded.rotation, 90);
        assert!(loaded.zoom > 0.0);
        assert!((loaded.scroll_frac - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recent_documents_are_ordered() {
        let db = db();
        let a = db.upsert_document("a", "/a.pdf", None, 1, 100).unwrap();
        let b = db.upsert_document("b", "/b.pdf", None, 1, 200).unwrap();
        let c = db.upsert_document("c", "/c.pdf", None, 1, 150).unwrap();
        let recent: Vec<i64> = db.recent(10).unwrap().iter().map(|d| d.id).collect();
        assert_eq!(recent, vec![b, c, a]);
        db.touch(a, 300).unwrap();
        let recent: Vec<i64> = db.recent(10).unwrap().iter().map(|d| d.id).collect();
        assert_eq!(recent, vec![a, b, c]);
        assert_eq!(db.recent(2).unwrap().len(), 2);
    }

    #[test]
    fn settings_round_trip() {
        let db = db();
        assert_eq!(db.get_setting("theme").unwrap(), None);
        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("dark"));
        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("light"));
        db.set_setting("window.w", "1024").unwrap();
        assert_eq!(db.setting_i64("window.w", 0), 1024);
        assert!(db.setting_bool("nonexistent", true));
    }

    #[test]
    fn highlights_round_trip() {
        let db = db();
        let id = db.upsert_document("h", "/h.pdf", None, 5, 1).unwrap();
        let rects = vec![Rect::new(10.0, 20.0, 100.0, 12.5), Rect::new(5.0, 40.0, 50.0, 12.5)];
        let hid = db
            .add_highlight(id, 2, &rects, 0xFFFF_C800, Some("important"), 42)
            .unwrap();
        let all = db.highlights(id).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, hid);
        assert_eq!(all[0].page, 2);
        assert_eq!(all[0].rects.len(), 2);
        assert!((all[0].rects[1].w - 50.0).abs() < 0.01);
        assert_eq!(all[0].note.as_deref(), Some("important"));
        db.delete_highlight(hid).unwrap();
        assert!(db.highlights(id).unwrap().is_empty());
        // Deleting the document cascades.
        db.add_highlight(id, 1, &rects, 0, None, 1).unwrap();
        db.forget_document(id).unwrap();
        assert!(db.highlights(id).unwrap().is_empty());
    }

    #[test]
    fn rect_codec_is_stable() {
        let rects = vec![Rect::new(1.0, 2.0, 3.0, 4.0), Rect::new(5.5, 6.5, 7.5, 8.5)];
        let encoded = encode_rects(&rects);
        assert_eq!(encoded, "1.00,2.00,3.00,4.00;5.50,6.50,7.50,8.50");
        let decoded = decode_rects(&encoded);
        assert_eq!(decoded, rects);
        assert!(decode_rects("garbage").is_empty());
    }

    #[test]
    fn reopening_the_same_file_keeps_the_session() {
        let dir = std::env::temp_dir().join(format!("pdfrust-db-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.db");
        let _ = std::fs::remove_file(&path);
        {
            let db = Database::open(&path).unwrap();
            let id = db.upsert_document("fp-x", "/x.pdf", None, 20, 1).unwrap();
            let session = SessionRow {
                last_page: 10,
                ..Default::default()
            };
            db.save_session(id, &session, 1).unwrap();
        }
        // Simulates an application restart (and a reboot: the file is on disk).
        {
            let db = Database::open(&path).unwrap();
            let row = db.find_by_fingerprint("fp-x").unwrap().unwrap();
            assert_eq!(row.session.last_page, 10);
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
