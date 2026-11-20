//! Tiny dynamic-library loader shared by the X11 shell and the runtime-loaded
//! SQLite bindings. Keeps the binary free of link-time dependencies on optional
//! system libraries.

use core::ffi::{c_char, c_int, c_void};

#[cfg(unix)]
mod imp {
    use super::*;

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
                    "unknown dynamic loader error".to_string()
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
mod imp {
    use super::*;
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

/// A loaded shared library. The handle is intentionally leaked: the process
/// needs the library for its whole life.
#[derive(Clone, Copy)]
pub struct Library {
    handle: *mut c_void,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("handle", &self.handle).finish()
    }
}

impl Library {
    pub fn open(names: &[&str]) -> Result<Library, String> {
        let mut last = String::new();
        for name in names {
            match imp::open(name) {
                Ok(handle) => return Ok(Library { handle }),
                Err(e) => last = format!("{}: {}", name, e),
            }
        }
        Err(last)
    }

    /// Resolves a symbol and transmutes it to the requested function type.
    ///
    /// # Safety
    /// The caller must know the exact C signature of the symbol.
    pub unsafe fn get<T: Copy>(&self, name: &str) -> Option<T> {
        let ptr = imp::sym(self.handle, name)?;
        debug_assert_eq!(
            std::mem::size_of::<T>(),
            std::mem::size_of::<*mut c_void>(),
            "function pointers are one pointer wide"
        );
        Some(std::mem::transmute_copy::<*mut c_void, T>(&ptr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn loads_libc_and_resolves_a_symbol() {
        let lib = Library::open(&["libc.so.6"]).expect("libc should load");
        // SAFETY: getpid has the well known libc signature.
        let getpid: Option<unsafe extern "C" fn() -> i32> = unsafe { lib.get("getpid") };
        assert!(getpid.is_some());
        let missing: Option<unsafe extern "C" fn() -> i32> = unsafe { lib.get("definitely_not_here") };
        assert!(missing.is_none());
    }

    #[test]
    fn missing_library_reports_the_name() {
        let err = Library::open(&["libdoes-not-exist-12345.so"]).unwrap_err();
        assert!(err.contains("libdoes-not-exist-12345.so"), "{err}");
    }
}
