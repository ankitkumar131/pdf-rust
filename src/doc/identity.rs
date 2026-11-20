//! Document identity.
//!
//! Reading progress must survive files being moved or renamed, so documents are
//! identified by a **content fingerprint**, not by their path:
//!
//! ```text
//! v1:<sha256( b"pdf-rust-v1" || size_le_u64 || first 64 KiB || last 64 KiB )>
//! ```
//!
//! For files up to 128 KiB the whole content is hashed (so the head and tail do
//! not overlap). Fingerprinting a 1 GB PDF therefore reads at most 128 KiB.
//!
//! The absolute path is stored alongside for display, "recent documents" and for
//! a fast path when reopening the same file, but it is never used as the identity
//! key. On Windows the stored path has the verbatim (`\\?\`) prefix removed: the
//! prefix is correct for file APIs but unreadable in the UI and it never matches a
//! path the user or the shell hands us later.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::util::hash::Sha256;

const CHUNK: u64 = 64 * 1024;
const FINGERPRINT_VERSION: &str = "v1";

/// Content-based document identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocIdentity {
    pub fingerprint: String,
    /// Canonical path when the file exists, otherwise the path as given.
    pub path: PathBuf,
    pub display_name: String,
    pub size: u64,
    /// Modification time in Unix seconds (used only for cache hints).
    pub modified: Option<u64>,
}

impl DocIdentity {
    pub fn path_string(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// Computes the fingerprint of a file.
pub fn fingerprint(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("could not open {}: {}", path.display(), e))?;
    let size = file
        .metadata()
        .map_err(|e| format!("could not stat {}: {}", path.display(), e))?
        .len();
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_VERSION.as_bytes());
    hasher.update(&size.to_le_bytes());
    if size <= CHUNK * 2 {
        let mut buf = Vec::with_capacity(size as usize);
        file.read_to_end(&mut buf)
            .map_err(|e| format!("could not read {}: {}", path.display(), e))?;
        hasher.update(&buf);
    } else {
        let mut head = vec![0u8; CHUNK as usize];
        file.read_exact(&mut head)
            .map_err(|e| format!("could not read {}: {}", path.display(), e))?;
        hasher.update(&head);
        let mut tail = vec![0u8; CHUNK as usize];
        file.seek(SeekFrom::Start(size - CHUNK))
            .map_err(|e| format!("could not seek in {}: {}", path.display(), e))?;
        file.read_exact(&mut tail)
            .map_err(|e| format!("could not read {}: {}", path.display(), e))?;
        hasher.update(&tail);
    }
    let digest = hasher.finish();
    let mut out = String::with_capacity(3 + 64);
    out.push_str(FINGERPRINT_VERSION);
    out.push(':');
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    Ok(out)
}

/// Absolute path of `path` in the form the UI and the database use.
///
/// `std::fs::canonicalize` is used when it succeeds because it resolves `.`, `..`
/// and symlinks. Windows returns a *verbatim* path (`\\?\C:\dir\file.pdf`) from
/// it, which is stripped here: the prefix is valid in Win32 calls but it would
/// show up in the UI, and a path stored with it never matches the plain path the
/// shell passes in later.
pub fn canonical_path(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    strip_verbatim(canonical)
}

/// Removes the Windows verbatim prefix: `\\?\C:\...` becomes `C:\...` and
/// `\\?\UNC\server\share` becomes `\\server\share`. A no-op elsewhere.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        return PathBuf::from(format!("\\\\{}", rest));
    }
    if let Some(rest) = text.strip_prefix("\\\\?\\") {
        return PathBuf::from(rest);
    }
    path
}

/// Full identity of a document on disk.
pub fn identify(path: &Path) -> Result<DocIdentity, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("could not open {}: {}", path.display(), e))?;
    if meta.is_dir() {
        return Err(format!("{} is a folder, not a PDF document", path.display()));
    }
    let canonical = canonical_path(path);
    let display_name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical.display().to_string());
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Ok(DocIdentity {
        fingerprint: fingerprint(&canonical)?,
        path: canonical,
        display_name,
        size: meta.len(),
        modified,
    })
}

/// Normalises a user supplied path (adds the current directory, trims quotes
/// that Windows "Open with" sometimes leaves in arguments).
pub fn normalise_arg(arg: &str) -> PathBuf {
    let trimmed = arg.trim().trim_matches('"');
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfrust-ident-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    #[test]
    fn identical_content_has_identical_fingerprint() {
        let data = b"%PDF-1.7 identical content".repeat(10);
        let a = temp_file("a.pdf", &data);
        let b = temp_file("b.pdf", &data);
        assert_eq!(fingerprint(&a).unwrap(), fingerprint(&b).unwrap());
    }

    #[test]
    fn different_content_differs() {
        let a = temp_file("c.pdf", &b"%PDF-1.7 one".repeat(10));
        let b = temp_file("d.pdf", &b"%PDF-1.7 two".repeat(10));
        assert_ne!(fingerprint(&a).unwrap(), fingerprint(&b).unwrap());
    }

    #[test]
    fn rename_keeps_the_fingerprint() {
        let path = temp_file("e.pdf", &b"%PDF-1.7 rename me".repeat(20));
        let before = fingerprint(&path).unwrap();
        let renamed = path.with_file_name("e-renamed.pdf");
        std::fs::rename(&path, &renamed).unwrap();
        assert_eq!(before, fingerprint(&renamed).unwrap());
    }

    #[test]
    fn large_file_uses_head_and_tail() {
        // 300 KiB: only the first/last 64 KiB matter, so a change in the middle
        // is allowed to (deliberately) go unnoticed — this documents the trade-off.
        let mut data = vec![b'a'; 300 * 1024];
        data[0] = b'X';
        let p1 = temp_file("f.pdf", &data);
        let fp1 = fingerprint(&p1).unwrap();
        data[0] = b'Y';
        let p2 = temp_file("g.pdf", &data);
        assert_ne!(fp1, fingerprint(&p2).unwrap());
        data[0] = b'X';
        let p3 = temp_file("h.pdf", &data);
        assert_eq!(fp1, fingerprint(&p3).unwrap());
    }

    #[test]
    fn canonical_path_is_absolute_and_prefix_free() {
        let path = temp_file("canon.pdf", b"%PDF-1.7 hello");
        let canonical = canonical_path(&path);
        assert!(canonical.is_absolute());
        assert!(!canonical.to_string_lossy().starts_with("\\\\?\\"));
        // A path that does not exist is returned as given (no panic, no escape).
        let missing = PathBuf::from("definitely-not-here/../nope.pdf");
        assert_eq!(canonical_path(&missing), missing);
    }

    #[test]
    fn identify_reports_metadata() {
        let path = temp_file("i.pdf", b"%PDF-1.7 hello");
        let ident = identify(&path).unwrap();
        assert!(ident.fingerprint.starts_with("v1:"));
        assert_eq!(ident.fingerprint.len(), 67);
        assert_eq!(ident.display_name, "i.pdf");
        assert_eq!(ident.size, 14);
        assert!(ident.path.is_absolute());
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = identify(Path::new("/definitely/not/here.pdf")).unwrap_err();
        assert!(err.to_lowercase().contains("could not open"));
    }

    #[test]
    fn arguments_are_normalised() {
        let p = normalise_arg("\"relative.pdf\"");
        assert!(p.is_absolute());
        assert!(p.to_string_lossy().ends_with("relative.pdf"));
    }
}
