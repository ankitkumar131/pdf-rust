//! Private temporary directories.
//!
//! Printing a rotated copy needs a scratch file on disk. Writing it into the
//! shared temp directory with a predictable name would let another user on the
//! machine read the document (or win a symlink race), so every scratch file
//! lives in a directory of its own that is created `0700` and removed as soon
//! as the job that owns it goes away.

use std::path::{Path, PathBuf};

/// A temporary directory owned by one operation.
///
/// The directory is created with owner-only permissions and removed recursively
/// when the guard is dropped — on success, on error and (because `App` holds it)
/// when the process exits.
pub struct PrivateTempDir {
    path: PathBuf,
}

impl PrivateTempDir {
    /// Creates `$TMPDIR/<tag>-<pid>-<counter>` with owner-only permissions.
    pub fn new(tag: &str) -> Result<PrivateTempDir, String> {
        let unique = unique_suffix();
        let path = std::env::temp_dir().join(format!(
            "pdf-rust-{}-{}-{}",
            sanitize(tag),
            std::process::id(),
            unique
        ));
        create_private_dir(&path).map_err(|e| format!("could not create a temporary directory: {}", e))?;
        Ok(PrivateTempDir { path })
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside the directory (the file itself is created by the writer).
    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(sanitize(name))
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl std::fmt::Debug for PrivateTempDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PrivateTempDir").field(&self.path).finish()
    }
}

/// Restricts an existing file to its owner (`0600` on Unix; a no-op elsewhere,
/// where the parent directory already excludes other users).
pub fn restrict_to_owner(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        // Touch the metadata so a typo'd path is reported the same way.
        std::fs::metadata(path).map(|_| ())
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(false).mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

/// Keeps only characters that are safe in a file name.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    if cleaned.is_empty() {
        "tmp".to_string()
    } else {
        cleaned
    }
}

/// A counter plus the wall clock, so two runs cannot collide.
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{}-{}", nanos, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_dir_is_private_and_removed_on_drop() {
        let path;
        {
            let dir = PrivateTempDir::new("test").expect("temp dir");
            path = dir.path().to_path_buf();
            assert!(path.is_dir());
            let file = dir.file("page.pdf");
            std::fs::write(&file, b"%PDF-1.7").expect("write");
            restrict_to_owner(&file).expect("chmod");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "scratch files are readable only by their owner");
                let dir_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(dir_mode, 0o700, "the directory is private");
            }
        }
        assert!(!path.exists(), "dropping the guard removes the directory");
    }

    #[test]
    fn names_inside_the_directory_are_sanitized() {
        let dir = PrivateTempDir::new("print/job").expect("temp dir");
        let file = dir.file("../../escape.pdf");
        assert_eq!(file.parent(), Some(dir.path()), "no path escapes the directory");
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains('/') && !name.contains('\\'), "one file name, no separators");
        assert!(!name.contains("..") || !name.contains('/'), "separators are replaced");
    }

    #[test]
    fn two_directories_never_collide() {
        let a = PrivateTempDir::new("x").unwrap();
        let b = PrivateTempDir::new("x").unwrap();
        assert_ne!(a.path(), b.path());
    }
}
