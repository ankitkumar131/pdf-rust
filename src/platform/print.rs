//! Printing, cross-platform.
//!
//! A [`PrintJob`] is a small state machine with one platform backend:
//!
//! * **Linux**: the file is handed to CUPS (`lp`, or `lpr`); no page images are
//!   needed because the print system rasterises the PDF itself.
//! * **Windows**: the reader renders each page at the printer resolution and
//!   sends it to the printer device context with GDI, after the standard print
//!   dialog. This is what makes printing work even when pdf-rust is the default
//!   PDF handler (no recursion, no external helper).
//!
//! The original file is never modified. Callers that need the user's rotation
//! applied export a rotated copy first and print that copy.

use std::path::Path;

use crate::ui::Bitmap;

/// An in-progress print job.
pub struct PrintJob {
    inner: imp::Job,
}

impl PrintJob {
    /// Starts a job for `path` (used for the title and by the file backend).
    pub fn start(path: &Path, title: &str) -> Result<PrintJob, String> {
        if !path.is_file() {
            return Err(format!("{} does not exist", path.display()));
        }
        Ok(PrintJob {
            inner: imp::Job::start(path, title)?,
        })
    }

    /// True when the caller must render pages and hand them over with [`Self::page`].
    pub fn needs_pages(&self) -> bool {
        self.inner.needs_pages()
    }

    /// Device pixels per PDF point that pages should be rendered at.
    pub fn scale(&self) -> f32 {
        self.inner.scale()
    }

    /// Sends one rendered page (in document order) to the printer.
    pub fn page(&mut self, bitmap: &Bitmap) -> Result<(), String> {
        self.inner.page(bitmap)
    }

    /// Completes the job and returns a user-facing message.
    pub fn finish(self) -> Result<String, String> {
        self.inner.finish()
    }
}

/// Sends `path` to the default printer without further interaction.
///
/// This is the simple path used by the command line and by tests; the UI uses a
/// [`PrintJob`] so it can apply rotation and show progress.
pub fn print_pdf(path: &Path, rotation: u16, all_pages: bool) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let _ = (rotation, all_pages);
    let job = PrintJob::start(path, "pdf-rust document")?;
    if job.needs_pages() {
        // The interactive Windows path is driven by the application, which owns
        // the rendered pages.
        return Err("printing from this entry point needs a window; use the Print button".into());
    }
    job.finish()
}

#[cfg(all(target_os = "linux", not(feature = "win32-syntax-check")))]
mod imp {
    use super::*;
    use std::process::{Command, Stdio};

    pub struct Job {
        path: std::path::PathBuf,
    }

    impl Job {
        pub fn start(path: &Path, _title: &str) -> Result<Job, String> {
            Ok(Job {
                path: path.to_path_buf(),
            })
        }

        pub fn needs_pages(&self) -> bool {
            false
        }

        pub fn scale(&self) -> f32 {
            1.0
        }

        pub fn page(&mut self, _bitmap: &Bitmap) -> Result<(), String> {
            Ok(())
        }

        pub fn finish(self) -> Result<String, String> {
            match run("lp", &self.path) {
                Ok(message) => Ok(message),
                Err(lp_error) => match run("lpr", &self.path) {
                    Ok(message) => Ok(message),
                    Err(lpr_error) => Err(format!(
                        "no print system available: lp failed ({}), lpr failed ({})",
                        lp_error, lpr_error
                    )),
                },
            }
        }
    }

    fn run(program: &str, path: &Path) -> Result<String, String> {
        let output = Command::new(program)
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{}: {}", program, e))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(if text.is_empty() {
                format!("Sent to the default printer with {}", program)
            } else {
                text
            })
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

#[cfg(any(windows, feature = "win32-syntax-check"))]
mod imp {
    pub use crate::platform::win32::printing::Job;
}

#[cfg(all(not(target_os = "linux"), not(windows), not(feature = "win32-syntax-check")))]
mod imp {
    use super::*;

    pub struct Job;

    impl Job {
        pub fn start(_path: &Path, _title: &str) -> Result<Job, String> {
            Err("printing is not supported on this platform".to_string())
        }

        pub fn needs_pages(&self) -> bool {
            false
        }

        pub fn scale(&self) -> f32 {
            1.0
        }

        pub fn page(&mut self, _bitmap: &Bitmap) -> Result<(), String> {
            Ok(())
        }

        pub fn finish(self) -> Result<String, String> {
            Ok("nothing to print".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printing_a_missing_file_fails_quickly() {
        let err = print_pdf(Path::new("/nonexistent/file.pdf"), 0, true).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }
}
