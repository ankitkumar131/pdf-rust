//! Platform shells and OS integration.
//!
//! The application core never touches a window system: each shell below owns a
//! native window, feeds events into [`crate::app::App`] and blits the finished
//! frame. Everything platform specific (X11, Win32) lives here.

pub mod dynload;
pub mod print;

#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(windows)]
pub mod win32;

/// URL schemes we are willing to hand to the operating system.
const ALLOWED_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// Returns true when `url` uses a scheme the reader considers safe.
pub fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ALLOWED_SCHEMES.iter().any(|p| lower.starts_with(p))
}

/// Opens an external link in the user's browser.
///
/// Only `http`, `https` and `mailto` links are accepted: a PDF cannot make the
/// reader launch arbitrary programs or `file:` / `javascript:` handlers.
pub fn shell_open(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !is_safe_url(url) {
        return Err(format!(
            "refusing to open a link that is not http, https or mailto: {}",
            url
        ));
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not run xdg-open: {}", e))
    }
    #[cfg(windows)]
    {
        win32::shell_execute(url)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err("opening links is not supported on this platform".to_string())
    }
}

/// Document paths given on the command line (the first non-flag argument may be
/// a page number, which the caller handles before calling this).
pub fn document_paths(args: &[String]) -> Vec<std::path::PathBuf> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(std::path::PathBuf::from)
        .collect()
}

/// Window title for a document set.
pub fn window_title(active_doc: Option<&str>) -> String {
    match active_doc {
        Some(name) => format!("{} — {}", name, crate::APP_NAME),
        None => format!("{} {}", crate::APP_NAME, crate::APP_VERSION),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_and_mail_links_are_allowed() {
        assert!(is_safe_url("https://example.com/doc"));
        assert!(is_safe_url("HTTP://example.com"));
        assert!(is_safe_url("mailto:someone@example.com"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("/usr/bin/eject"));
        assert!(!is_safe_url("cmd.exe"));
        assert!(shell_open("javascript:alert(1)").is_err());
    }

    #[test]
    fn window_titles_include_the_document() {
        assert_eq!(window_title(Some("manual.pdf")), "manual.pdf — pdf-rust");
        assert!(window_title(None).starts_with("pdf-rust"));
    }

    #[test]
    fn command_line_paths_skip_flags() {
        let args = vec!["--db".to_string(), "/tmp/a.pdf".to_string(), "b.pdf".to_string()];
        let paths = document_paths(&args);
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("a.pdf"));
    }
}
