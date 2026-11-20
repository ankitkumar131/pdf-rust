//! Minimal, safe SQLite bindings loaded at runtime.
//!
//! We only need a dozen entry points, so instead of linking `libsqlite3` we open
//! the system library with `dlopen`/`LoadLibraryW`:
//!
//! * Linux: `libsqlite3.so.0` (present on every desktop distribution) or a
//!   bundled copy in `runtime/sqlite/`.
//! * Windows: `winsqlite3.dll`, which ships with Windows 10/11 and exports the
//!   standard `sqlite3_*` symbols, or a bundled `sqlite3.dll`.
//!
//! This keeps the build free of C compilation and of `-lsqlite3` link-time
//! requirements, and lets the application report a clean error instead of
//! refusing to start.

use core::ffi::{c_char, c_int, c_uchar, c_void};
use std::path::{Path, PathBuf};

pub const SQLITE_OK: c_int = 0;
pub const SQLITE_ROW: c_int = 100;
pub const SQLITE_DONE: c_int = 101;

const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;

type Handle = *mut c_void;
type RawStmt = *mut c_void;

macro_rules! declare_api {
    ($( fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty ; )*) => {
        pub struct Api {
            $( $name: unsafe extern "C" fn($($ty),*) -> $ret, )*
        }
        impl Api {
            fn load(get: &dyn Fn(&str) -> Option<*mut c_void>) -> Result<Api, String> {
                Ok(Api {
                    $( $name: {
                        let sym = concat!(stringify!($name), "\0");
                        let ptr = get(sym).ok_or_else(|| {
                            format!("the SQLite library is missing the symbol {}", stringify!($name))
                        })?;
                        // SAFETY: symbol comes from SQLite with the documented signature.
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($ty),*) -> $ret>(ptr) }
                    }, )*
                })
            }
        }
    };
}

declare_api! {
    fn sqlite3_open_v2(filename: *const c_char, db: *mut Handle, flags: c_int, vfs: *const c_char) -> c_int;
    fn sqlite3_close_v2(db: Handle) -> c_int;
    fn sqlite3_errmsg(db: Handle) -> *const c_char;
    fn sqlite3_exec(db: Handle, sql: *const c_char, cb: *mut c_void, arg: *mut c_void, err: *mut *mut c_char) -> c_int;
    fn sqlite3_prepare_v2(db: Handle, sql: *const c_char, n: c_int, stmt: *mut RawStmt, tail: *mut *const c_char) -> c_int;
    fn sqlite3_step(stmt: RawStmt) -> c_int;
    fn sqlite3_finalize(stmt: RawStmt) -> c_int;
    fn sqlite3_reset(stmt: RawStmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: RawStmt) -> c_int;
    fn sqlite3_bind_text(stmt: RawStmt, index: c_int, text: *const c_char, len: c_int, destructor: *const c_void) -> c_int;
    fn sqlite3_bind_int64(stmt: RawStmt, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_double(stmt: RawStmt, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_null(stmt: RawStmt, index: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: RawStmt, col: c_int) -> i64;
    fn sqlite3_column_double(stmt: RawStmt, col: c_int) -> f64;
    fn sqlite3_column_text(stmt: RawStmt, col: c_int) -> *const c_uchar;
    fn sqlite3_column_bytes(stmt: RawStmt, col: c_int) -> c_int;
    fn sqlite3_column_type(stmt: RawStmt, col: c_int) -> c_int;
    fn sqlite3_last_insert_rowid(db: Handle) -> i64;
    fn sqlite3_changes(db: Handle) -> c_int;
}

const SQLITE_NULL: c_int = 5;

#[cfg(unix)]
mod dynlib {
    use core::ffi::{c_char, c_int, c_void};

    #[link(name = "dl")]
    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    pub fn open(path: &str) -> Result<*mut c_void, String> {
        let c = std::ffi::CString::new(path).map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated path from a valid CString.
        let handle = unsafe { dlopen(c.as_ptr(), 2) };
        if handle.is_null() {
            // SAFETY: dlerror returns a NUL-terminated message or null.
            let msg = unsafe {
                let err = dlerror();
                if err.is_null() {
                    "unknown dynamic loader error".into()
                } else {
                    std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
                }
            };
            return Err(msg);
        }
        Ok(handle)
    }

    pub fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c = std::ffi::CString::new(name.trim_end_matches('\0')).ok()?;
        // SAFETY: handle comes from dlopen.
        let p = unsafe { dlsym(handle, c.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

#[cfg(windows)]
mod dynlib {
    use core::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub fn open(path: &str) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: NUL-terminated UTF-16 path.
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            return Err(format!("LoadLibraryW failed for {}", path));
        }
        Ok(handle)
    }

    pub fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let mut bytes = name.trim_end_matches('\0').as_bytes().to_vec();
        bytes.push(0);
        // SAFETY: NUL-terminated ASCII symbol name.
        let p = unsafe { GetProcAddress(handle, bytes.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

static API: std::sync::OnceLock<Result<&'static Api, String>> = std::sync::OnceLock::new();

/// Returns the process-wide SQLite API, loading the library on first use.
pub fn api() -> Result<&'static Api, String> {
    API.get_or_init(|| {
        let mut errors = Vec::new();
        for candidate in search_paths() {
            match dynlib::open(&candidate.to_string_lossy()) {
                Ok(handle) => match Api::load(&|name| dynlib::sym(handle, name)) {
                    Ok(api) => return Ok(Box::leak(Box::new(api))),
                    Err(e) => errors.push(format!("{}: {}", candidate.display(), e)),
                },
                Err(e) => errors.push(format!("{}: {}", candidate.display(), e)),
            }
        }
        Err(format!(
            "SQLite could not be loaded (tried {}). Set PDFRUST_SQLITE to a sqlite3 shared library.",
            errors.join("; ")
        ))
    })
    .clone()
}

/// True when a SQLite shared library is reachable.
pub fn available() -> bool {
    api().is_ok()
}

pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("PDFRUST_SQLITE") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["winsqlite3.dll", "sqlite3.dll", "sqlite3_64.dll"]
    } else {
        &["libsqlite3.so.0", "libsqlite3.so", "sqlite3"]
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in names {
                out.push(dir.join(name));
                out.push(dir.join("runtime").join("sqlite").join(name));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for name in names {
            out.push(cwd.join("runtime").join("sqlite").join(name));
        }
    }
    if cfg!(windows) {
        // The system copy always wins on Windows because it is in System32.
        for name in names {
            out.push(PathBuf::from(name));
        }
    } else {
        for name in names {
            out.push(PathBuf::from(name));
            out.push(PathBuf::from("/usr/lib").join(name));
            out.push(PathBuf::from("/usr/lib/x86_64-linux-gnu").join(name));
            out.push(PathBuf::from("/usr/local/lib").join(name));
        }
    }
    out
}

/// A connection to a SQLite database.
pub struct Sqlite {
    api: &'static Api,
    db: Handle,
}

impl Sqlite {
    pub fn open(path: &Path) -> Result<Sqlite, String> {
        let api = api()?;
        let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
            .map_err(|e| format!("invalid database path: {}", e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {}", parent.display(), e))?;
        }
        let mut db: Handle = std::ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX;
        // SAFETY: valid path and out-pointer.
        let rc = unsafe { (api.sqlite3_open_v2)(c_path.as_ptr(), &mut db, flags, std::ptr::null()) };
        if rc != SQLITE_OK || db.is_null() {
            let msg = if db.is_null() {
                "SQLite could not allocate a connection".to_string()
            } else {
                // SAFETY: db is a valid connection.
                unsafe { errmsg(api, db) }
            };
            if !db.is_null() {
                // SAFETY: closing a failed connection is allowed.
                unsafe { (api.sqlite3_close_v2)(db) };
            }
            return Err(msg);
        }
        let conn = Sqlite { api, db };
        conn.exec("PRAGMA busy_timeout = 5000")?;
        conn.exec("PRAGMA foreign_keys = ON")?;
        // WAL keeps writes transactional and crash-safe without blocking reads.
        let _ = conn.exec("PRAGMA journal_mode = WAL");
        conn.exec("PRAGMA synchronous = NORMAL")?;
        Ok(conn)
    }

    pub fn exec(&self, sql: &str) -> Result<(), String> {
        let c_sql = std::ffi::CString::new(sql).map_err(|e| e.to_string())?;
        // SAFETY: valid connection and NUL-terminated SQL; no callback.
        let rc = unsafe {
            (self.api.sqlite3_exec)(
                self.db,
                c_sql.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != SQLITE_OK {
            return Err(self.last_error());
        }
        Ok(())
    }

    pub fn prepare(&self, sql: &str) -> Result<Stmt<'_>, String> {
        let c_sql = std::ffi::CString::new(sql).map_err(|e| e.to_string())?;
        let mut stmt: RawStmt = std::ptr::null_mut();
        // SAFETY: valid connection; statement out-pointer initialised to null.
        let rc = unsafe {
            (self.api.sqlite3_prepare_v2)(
                self.db,
                c_sql.as_ptr(),
                -1,
                &mut stmt,
                std::ptr::null_mut(),
            )
        };
        if rc != SQLITE_OK || stmt.is_null() {
            return Err(self.last_error());
        }
        Ok(Stmt {
            api: self.api,
            db: self.db,
            stmt,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn last_error(&self) -> String {
        // SAFETY: valid connection handle.
        unsafe { errmsg(self.api, self.db) }
    }

    pub fn last_insert_rowid(&self) -> i64 {
        // SAFETY: valid connection handle.
        unsafe { (self.api.sqlite3_last_insert_rowid)(self.db) }
    }

    pub fn changes(&self) -> i32 {
        // SAFETY: valid connection handle.
        unsafe { (self.api.sqlite3_changes)(self.db) }
    }
}

impl Drop for Sqlite {
    fn drop(&mut self) {
        if !self.db.is_null() {
            // SAFETY: connection is owned by this struct and closed once.
            unsafe { (self.api.sqlite3_close_v2)(self.db) };
        }
    }
}

unsafe fn errmsg(api: &Api, db: Handle) -> String {
    if db.is_null() {
        return "unknown SQLite error".to_string();
    }
    let ptr = (api.sqlite3_errmsg)(db);
    if ptr.is_null() {
        "unknown SQLite error".to_string()
    } else {
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// A prepared statement.
pub struct Stmt<'a> {
    api: &'static Api,
    db: Handle,
    stmt: RawStmt,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Stmt<'a> {
    pub fn bind_text(&mut self, index: i32, value: &str) {
        // SAFETY: statement handle is valid; SQLITE_TRANSIENT (-1) copies the
        // buffer so the borrow does not outlive the call.
        unsafe {
            (self.api.sqlite3_bind_text)(
                self.stmt,
                index,
                value.as_ptr() as *const c_char,
                value.len() as c_int,
                (-1isize) as *const c_void,
            );
        }
    }

    pub fn bind_i64(&mut self, index: i32, value: i64) {
        // SAFETY: valid statement handle.
        unsafe {
            (self.api.sqlite3_bind_int64)(self.stmt, index, value);
        }
    }

    pub fn bind_f64(&mut self, index: i32, value: f64) {
        // SAFETY: valid statement handle.
        unsafe {
            (self.api.sqlite3_bind_double)(self.stmt, index, value);
        }
    }

    pub fn bind_null(&mut self, index: i32) {
        // SAFETY: valid statement handle.
        unsafe {
            (self.api.sqlite3_bind_null)(self.stmt, index);
        }
    }

    /// Advances the statement: `true` when a row is available.
    pub fn step(&mut self) -> Result<bool, String> {
        // SAFETY: valid statement handle.
        let rc = unsafe { (self.api.sqlite3_step)(self.stmt) };
        match rc {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            _ => // SAFETY: valid connection handle owned by the caller's Sqlite.
            Err(unsafe { errmsg(self.api, self.db) }),
        }
    }

    pub fn column_text(&self, col: i32) -> String {
        // SAFETY: valid statement handle; SQLite owns the returned buffer until
        // the next step, and we copy it immediately.
        unsafe {
            let ptr = (self.api.sqlite3_column_text)(self.stmt, col);
            if ptr.is_null() {
                return String::new();
            }
            let len = (self.api.sqlite3_column_bytes)(self.stmt, col).max(0) as usize;
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    }

    pub fn column_i64(&self, col: i32) -> i64 {
        // SAFETY: valid statement handle.
        unsafe { (self.api.sqlite3_column_int64)(self.stmt, col) }
    }

    pub fn column_f64(&self, col: i32) -> f64 {
        // SAFETY: valid statement handle.
        unsafe { (self.api.sqlite3_column_double)(self.stmt, col) }
    }

    pub fn is_null(&self, col: i32) -> bool {
        // SAFETY: valid statement handle.
        unsafe { (self.api.sqlite3_column_type)(self.stmt, col) == SQLITE_NULL }
    }

    pub fn reset(&mut self) {
        // SAFETY: valid statement handle.
        unsafe {
            (self.api.sqlite3_reset)(self.stmt);
            (self.api.sqlite3_clear_bindings)(self.stmt);
        }
    }
}

impl<'a> Drop for Stmt<'a> {
    fn drop(&mut self) {
        if !self.stmt.is_null() {
            // SAFETY: statement is owned by this struct.
            unsafe { (self.api.sqlite3_finalize)(self.stmt) };
        }
    }
}

/// Runs `f` inside `BEGIN IMMEDIATE ... COMMIT`, rolling back on error.
pub fn transaction<T>(
    conn: &Sqlite,
    f: impl FnOnce(&Sqlite) -> Result<T, String>,
) -> Result<T, String> {
    conn.exec("BEGIN IMMEDIATE")?;
    match f(conn) {
        Ok(value) => match conn.exec("COMMIT") {
            Ok(()) => Ok(value),
            Err(e) => {
                let _ = conn.exec("ROLLBACK");
                Err(e)
            }
        },
        Err(e) => {
            let _ = conn.exec("ROLLBACK");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_discoverable_on_this_system() {
        // Not every machine has SQLite, so this only asserts no panic.
        let _ = available();
        assert!(!search_paths().is_empty());
    }

    #[test]
    fn basic_round_trip() {
        if !available() {
            eprintln!("skipping: SQLite not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pdfrust-sql-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let conn = Sqlite::open(&path).unwrap();
        conn.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, score REAL)").unwrap();
        {
            let mut stmt = conn.prepare("INSERT INTO t (name, score) VALUES (?1, ?2)").unwrap();
            stmt.bind_text(1, "hello");
            stmt.bind_f64(2, 1.5);
            assert!(!stmt.step().unwrap());
        }
        assert_eq!(conn.last_insert_rowid(), 1);
        {
            let mut stmt = conn.prepare("SELECT name, score FROM t WHERE id = 1").unwrap();
            assert!(stmt.step().unwrap());
            assert_eq!(stmt.column_text(0), "hello");
            assert!((stmt.column_f64(1) - 1.5).abs() < 1e-9);
            assert!(!stmt.step().unwrap());
        }
        assert!(conn.prepare("SELECT * FROM missing_table").is_err());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        if !available() {
            eprintln!("skipping: SQLite not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pdfrust-tx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let conn = Sqlite::open(&path).unwrap();
        conn.exec("CREATE TABLE t (id INTEGER PRIMARY KEY)").unwrap();
        let result: Result<(), String> = transaction(&conn, |c| {
            c.exec("INSERT INTO t DEFAULT VALUES")?;
            Err("boom".to_string())
        });
        assert!(result.is_err());
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM t").unwrap();
        stmt.step().unwrap();
        assert_eq!(stmt.column_i64(0), 0, "insert should have been rolled back");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
