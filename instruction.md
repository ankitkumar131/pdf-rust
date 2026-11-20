You are a senior Rust desktop application architect and engineer. Build a production-quality, lightweight, high-performance PDF reader for Windows (primary target) and Linux (secondary target).

PROJECT GOAL:
Create a native-feeling PDF reader using Rust. It must open PDFs quickly, consume minimal RAM and disk space, restore the exact reading position after application restarts or system reboots, and integrate with Windows and Linux as a default PDF-handling application.

TECHNOLOGY:
- Rust for core application logic.
- Evaluate eframe/egui, Tauri, and native alternatives before selecting the architecture. Prefer the smallest practical runtime and avoid unnecessary webview overhead.
- Use a reliable PDF rendering library such as PDFium (with an appropriately packaged runtime) or another actively maintained Rust-compatible engine. Evaluate licensing, binary size, rendering quality, text extraction, and Windows/Linux support.
- Use SQLite for persistent reading history and settings.
- Use platform-specific integration code only where necessary.
- Use cargo workspaces only if they add architectural value.

CORE FEATURES:

1. PDF OPENING
- Open PDFs through a file picker.
- Support drag and drop.
- Open PDFs through double-clicking files in Windows Explorer and Linux file managers.
- Support command-line arguments to open a PDF path.
- Support opening multiple PDFs in separate tabs or windows.
- Handle large PDFs without loading every page into RAM.
- Handle invalid, encrypted, and corrupted PDFs gracefully.

2. READING POSITION PERSISTENCE — CRITICAL
- Save the last-read page for every PDF.
- When a user closes a PDF and opens it again, automatically restore the last page.
- Restoration must survive application closure, crashes where possible, and full computer restarts.
- Store document identity using canonical file path plus file metadata and a robust fingerprint where practical.
- Save progress periodically, after navigation, and on application shutdown.
- Save scroll position within a page if continuous scrolling is supported.
- Restore zoom level, rotation, and reading mode per document.
- Do not reset reading progress when a PDF is moved or renamed if its identity can be reliably matched.
- Provide a setting to ask whether to resume or start from the beginning.
- Provide a recent documents list.

3. PDF NAVIGATION
- Next page and previous page.
- Go to a specific page.
- Display current page / total pages (e.g. 10 / 20).
- First page and last page.
- Keyboard shortcuts.
- Page thumbnails sidebar.
- Table of contents / bookmarks navigation when available.
- Continuous scrolling and single-page modes.

4. ZOOM
- Zoom in and zoom out.
- Fit to width.
- Fit to page.
- Actual size (100%).
- Custom zoom percentages.
- Ctrl + mouse wheel zoom.
- Remember zoom per document.
- Keep the visible reading position stable when zooming.

5. ROTATION
- Rotate clockwise by 90 degrees.
- Rotate counterclockwise by 90 degrees.
- Support 180-degree rotation.
- Rotation should apply to the current page or all pages according to user choice.
- Remember rotation preferences between sessions.
- Never modify the original PDF unless the user explicitly exports a rotated copy.

6. READING EXPERIENCE
- Light and dark themes.
- Minimal, clean toolbar.
- Full-screen reading mode.
- Presentation mode.
- Adjustable sidebar.
- Smooth scrolling.
- Page transition controls.
- Text selection and copying.
- Search text within PDFs.
- Highlight and annotation support if feasible.
- Find next / previous search result.
- Print PDFs.
- Open external PDF links safely.

7. PERFORMANCE
- Prioritize fast startup and low memory usage.
- Lazy-load PDF pages.
- Render only visible pages and nearby prefetch pages.
- Implement a bounded rendering cache.
- Use background rendering tasks.
- Avoid blocking the UI thread.
- Use asynchronous operations where beneficial.
- Release unused page textures and buffers.
- Optimize release builds using LTO, codegen-units, and stripping where appropriate.
- Avoid unnecessary dependencies.
- Measure startup time, memory usage, and binary size.
- Do not sacrifice PDF rendering correctness for speed.

8. OS INTEGRATION
WINDOWS:
- Register the application as a PDF opener.
- Provide proper Windows installer.
- Support Windows Explorer Open With.
- Allow users to select it as the default PDF application.
- Set appropriate file associations and MIME/application metadata.
- Support application icons and version metadata.
- Handle file-open command-line arguments correctly.
- Support Windows 10 and Windows 11.
- Test installation, upgrades, uninstalling, and default application registration.

LINUX:
- Provide .desktop entry.
- Register application/x-pdf and application/pdf MIME types.
- Provide MIME associations and application icons.
- Support Open With and default PDF application selection.
- Provide an AppImage and/or .deb package where practical.
- Test on Ubuntu/Debian-based systems and Zorin OS.

9. DATABASE AND USER SETTINGS
Use SQLite to store:
- Document history.
- Last-read page.
- Scroll position.
- Zoom level.
- Rotation.
- Reading mode.
- Last opened timestamp.
- Application preferences.
- Window dimensions and position.

Database must be versioned and support migrations.
Use safe, transactional writes.
Avoid storing entire PDF documents in the database.

10. SECURITY AND PRIVACY
- All processing should be local.
- No analytics or telemetry by default.
- No unnecessary internet requests.
- Handle malformed PDFs defensively.
- Avoid executing embedded JavaScript or unsafe PDF actions.
- Never modify original documents without explicit user permission.

11. UI/UX
- Modern, minimalist desktop design.
- Native-feeling Windows interface.
- Responsive layout.
- Keyboard-first navigation.
- Clear icons and tooltips.
- Fast document opening.
- No unnecessary animations.
- Proper error messages and loading indicators.
- Consistent UI on Windows and Linux.

Suggested shortcuts:
- Ctrl + O: Open PDF.
- Ctrl + Plus: Zoom in.
- Ctrl + Minus: Zoom out.
- Ctrl + 0: Fit page or reset zoom.
- Left Arrow: Previous page.
- Right Arrow: Next page.
- Ctrl + F: Search.
- Ctrl + P: Print.
- F11: Full screen.
- Ctrl + R: Rotate clockwise.
- Ctrl + W: Close document.

12. BUILD AND DISTRIBUTION
Windows:
- Optimized release executable.
- Installer using a suitable lightweight installer technology.
- Portable ZIP version.
- Proper executable icon and metadata.

Linux:
- AppImage.
- Debian package where practical.
- Optimized release executable.

Keep installers and dependencies as small as practical. Report actual installed size and download size.

13. TESTING
- Unit tests for database and reading-progress persistence.
- Integration tests for PDF opening and navigation.
- Test reopening the same PDF after application closure.
- Test reopening after computer restart.
- Test multiple documents.
- Test documents with hundreds or thousands of pages.
- Test encrypted and corrupted documents.
- Test zoom and rotation persistence.
- Test file associations on Windows and Linux.
- Test memory usage during extended reading sessions.

14. GIT AND CONTINUOUS DEVELOPMENT — MANDATORY
- First inspect the existing repository before changing anything.
- If no repository exists, initialize Git and configure the project.
- Keep pushing code continuously to the configured remote repository.
- Do not wait until the entire project is finished to make the first push.
- Work in small, independently verifiable milestones.
- After each completed milestone:
  1. Format the code.
  2. Run relevant tests.
  3. Run cargo check and cargo build.
  4. Review the Git diff.
  5. Commit the working changes.
  6. Push to the remote repository.
- Use meaningful commit messages.
- Never commit secrets, local databases, build outputs, or personal PDF documents.
- Add a proper .gitignore.
- Never force-push, rewrite shared history, or overwrite existing work without permission.
- Do not claim a push succeeded unless Git confirms it.
- If pushing fails due to authentication or network issues, explain the problem and retain the local commits.
- Maintain a README with setup, architecture, build, installation, and usage instructions.
- Maintain a CHANGELOG.md.
- Maintain a DEVELOPMENT.md containing progress, pending features, known bugs, and next milestones.

15. DEVELOPMENT MILESTONES

Milestone 1: Project initialization, architecture, Git setup, basic application window.

Milestone 2: PDF rendering engine and opening local PDF documents.

Milestone 3: Navigation, page counter, zoom, rotation.

Milestone 4: SQLite-based reading position persistence and restoration.

Milestone 5: Thumbnails, bookmarks, text search, and recent documents.

Milestone 6: UI polish, themes, keyboard shortcuts, and performance optimization.

Milestone 7: Windows installer, portable version, and file associations.

Milestone 8: Linux packaging and MIME registration.

Milestone 9: Cross-platform testing, security review, and release preparation.

16. ACCEPTANCE CRITERIA
The application is not complete until:
- A PDF can be opened through the UI and by double-clicking in Windows Explorer.
- A 20-page PDF opened on page 10 restores page 10 after closing and reopening.
- Page restoration also works after a complete laptop restart.
- Zoom and rotation settings persist.
- Large PDFs can be read without excessive memory usage.
- The application can open multiple PDFs.
- Windows installer and portable distribution work.
- Linux package works.
- The application is recognized as a PDF reader by both operating systems.
- All critical features have automated or documented manual tests.
- Source code is pushed to the configured remote Git repository.

17. FINAL INSTRUCTIONS
Do not build a prototype and declare it finished.
Do not leave fake buttons or non-functional features.
Do not skip error handling.
Do not add unnecessary dependencies.
Do not prioritize visual effects over performance.
Do not silently remove existing functionality.
Do not stop after each milestone to ask for approval unless a genuine blocker or destructive action requires it.

Start by inspecting the repository, evaluating the rendering engine and UI framework, writing a short architecture decision record, and implementing Milestone 1. Continue through the milestones systematically, committing and pushing verified work throughout development.
