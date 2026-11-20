//! Printing.
//!
//! The original file is never modified. If the reader's rotation differs from
//! the file's own rotation, the caller exports a rotated copy first and prints
//! that copy; otherwise the file is sent to the print system as it is.
//!
//! * Linux: CUPS through `lp` (or `lpr` when `lp` is missing).
//! * Windows: the shell's `print` verb, which opens the application registered
//!   for PDF printing. The reader does not become the print handler for itself:
//!   it prints through the OS so scaling and printer selection are handled by
//!   the platform dialog.

use std::path::Path;
use std::process::{Command, Stdio};

/// Sends `path` to the default printer.
///
/// `rotation`/`all_pages` describe how the user is currently viewing the
/// document; when they differ from the file the caller is expected to have
/// exported a rotated copy already (see `App::print_document`).
pub fn print_pdf(path: &Path, rotation: u16, all_pages: bool) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let _ = (rotation, all_pages);
    #[cfg(target_os = "linux")]
    {
        linux::print(path)
    }
    #[cfg(windows)]
    {
        crate::platform::win32::print_file(path)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err("printing is not supported on this platform".to_string())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub fn print(path: &Path) -> Result<String, String> {
        match run("lp", path) {
            Ok(message) => Ok(message),
            Err(lp_error) => match run("lpr", path) {
                Ok(message) => Ok(message),
                Err(lpr_error) => Err(format!(
                    "no print system available: lp failed ({}), lpr failed ({})",
                    lp_error, lpr_error
                )),
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printing_a_missing_file_fails_quickly() {
        let err = print_pdf(Path::new("/nonexistent/file.pdf"), 0, true).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }
}
