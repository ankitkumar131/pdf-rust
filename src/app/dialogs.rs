//! State for modal dialogs (file picker, go-to-page, settings, resume prompt,
//! password prompt, document info and errors).

use std::path::{Path, PathBuf};

use crate::geom::Rect;
use crate::ui::widgets::TextField;

/// What the reader does when a document with saved progress is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResumePolicy {
    /// Ask every time progress is found.
    #[default]
    Ask,
    /// Always restore silently.
    Always,
    /// Always start from the first page.
    Never,
}

impl ResumePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ResumePolicy::Ask => "ask",
            ResumePolicy::Always => "always",
            ResumePolicy::Never => "never",
        }
    }

    pub fn parse(s: &str) -> ResumePolicy {
        match s {
            "always" => ResumePolicy::Always,
            "never" => ResumePolicy::Never,
            _ => ResumePolicy::Ask,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ResumePolicy::Ask => "Ask me",
            ResumePolicy::Always => "Always resume",
            ResumePolicy::Never => "Always start from the beginning",
        }
    }
}

/// One row of the built-in file browser.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// The built-in open dialog (used on both platforms so the UI is identical).
#[derive(Debug)]
pub struct FileDialog {
    pub dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: Option<usize>,
    pub path_field: TextField,
    pub error: Option<String>,
    pub scroll: f32,
    pub show_all_files: bool,
    pub focus_list: bool,
}

impl FileDialog {
    pub fn new(start: Option<PathBuf>) -> FileDialog {
        let dir = start
            .filter(|p| p.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut dialog = FileDialog {
            dir,
            entries: Vec::new(),
            selected: None,
            path_field: TextField::default(),
            error: None,
            scroll: 0.0,
            show_all_files: false,
            focus_list: true,
        };
        dialog.refresh();
        dialog
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.scroll = 0.0;
        self.path_field = TextField::with_text(&self.dir.to_string_lossy());
        let read = match std::fs::read_dir(&self.dir) {
            Ok(read) => read,
            Err(e) => {
                self.error = Some(format!("Cannot open {}: {}", self.dir.display(), e));
                return;
            }
        };
        self.error = None;
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.is_dir() {
                dirs.push(FileEntry {
                    name,
                    path,
                    is_dir: true,
                    size: 0,
                });
            } else {
                let is_pdf = path
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("pdf"))
                    .unwrap_or(false);
                if is_pdf || self.show_all_files {
                    files.push(FileEntry {
                        name,
                        path,
                        is_dir: false,
                        size: meta.len(),
                    });
                }
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        dirs.append(&mut files);
        self.entries = dirs;
        if !self.entries.is_empty() {
            self.selected = Some(0);
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.dir.parent() {
            self.dir = parent.to_path_buf();
            self.refresh();
        }
    }

    pub fn navigate_to(&mut self, path: &Path) {
        if path.is_dir() {
            self.dir = path.to_path_buf();
            self.refresh();
        } else if let Some(parent) = path.parent() {
            self.dir = parent.to_path_buf();
            self.refresh();
            if path.is_file() {
                self.selected = self.entries.iter().position(|e| e.path == path);
            }
        }
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    pub fn activate_selected(&mut self) -> Option<PathBuf> {
        let entry = self.selected_entry()?.clone();
        if entry.is_dir {
            self.dir = entry.path;
            self.refresh();
            None
        } else {
            Some(entry.path)
        }
    }
}

#[derive(Debug, Default)]
pub struct GotoDialog {
    pub field: TextField,
}

#[derive(Debug)]
pub struct ResumeDialog {
    pub doc_index: usize,
    pub page: u32,
    pub total: u32,
    pub remember: bool,
}

#[derive(Debug)]
pub struct PasswordDialog {
    pub doc_index: usize,
    pub document: String,
    pub field: TextField,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ErrorDialog {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
}

/// All modal state. Only one "primary" dialog is visible at a time; the error
/// dialog can stack on top of anything else.
#[derive(Default)]
pub struct Dialogs {
    pub open: Option<FileDialog>,
    pub goto: Option<GotoDialog>,
    pub resume: Option<ResumeDialog>,
    pub password: Option<PasswordDialog>,
    pub settings: bool,
    pub info: bool,
    pub shortcuts: bool,
    pub error: Option<ErrorDialog>,
    /// Window geometry saved before entering full screen.
    pub restore_geometry: Option<Rect>,
}

impl Dialogs {
    /// True when a modal is on screen (and therefore blocks interaction below).
    pub fn any(&self) -> bool {
        self.open.is_some()
            || self.goto.is_some()
            || self.resume.is_some()
            || self.password.is_some()
            || self.settings
            || self.info
            || self.shortcuts
            || self.error.is_some()
    }

    pub fn error(&mut self, title: &str, message: impl Into<String>) {
        self.error = Some(ErrorDialog {
            title: title.to_string(),
            message: message.into(),
            detail: None,
        });
    }

    pub fn close_all(&mut self) {
        self.open = None;
        self.goto = None;
        self.resume = None;
        self.password = None;
        self.settings = false;
        self.info = false;
        self.shortcuts = false;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_policy_round_trips() {
        for policy in [ResumePolicy::Ask, ResumePolicy::Always, ResumePolicy::Never] {
            assert_eq!(ResumePolicy::parse(policy.as_str()), policy);
            assert!(!policy.label().is_empty());
        }
        assert_eq!(ResumePolicy::parse("nonsense"), ResumePolicy::Ask);
    }

    #[test]
    fn file_dialog_lists_pdfs_and_directories() {
        let dir = std::env::temp_dir().join(format!("pdfrust-dialog-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("doc.pdf"), b"%PDF-1.7").unwrap();
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        let mut dialog = FileDialog::new(Some(dir.clone()));
        let names: Vec<&str> = dialog.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"sub"));
        assert!(names.contains(&"doc.pdf"));
        assert!(!names.contains(&"notes.txt"), "non-PDFs are filtered out");
        assert!(dialog.entries[0].is_dir, "directories come first");
        dialog.show_all_files = true;
        dialog.refresh();
        assert!(dialog.entries.iter().any(|e| e.name == "notes.txt"));
        dialog.go_up();
        assert_ne!(dialog.dir, dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn activating_a_directory_navigates_into_it() {
        let dir = std::env::temp_dir().join(format!("pdfrust-dialog2-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        let mut dialog = FileDialog::new(Some(dir.clone()));
        dialog.selected = dialog.entries.iter().position(|e| e.name == "inner");
        assert_eq!(dialog.activate_selected(), None);
        assert!(dialog.dir.ends_with("inner"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dialogs_report_modality() {
        let mut dialogs = Dialogs::default();
        assert!(!dialogs.any());
        dialogs.error("Oops", "something failed");
        assert!(dialogs.any());
        dialogs.close_all();
        assert!(!dialogs.any());
    }
}
