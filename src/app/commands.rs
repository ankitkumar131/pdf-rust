//! User commands and the keyboard map.

use crate::ui::{Input, Key};

/// Everything the user can ask the application to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    OpenDialog,
    CloseTab,
    Quit,
    NextPage,
    PrevPage,
    FirstPage,
    LastPage,
    NextDocument,
    PrevDocument,
    GoToPageDialog,
    ZoomIn,
    ZoomOut,
    FitPage,
    FitWidth,
    ZoomActual,
    RotateCw,
    RotateCcw,
    Rotate180,
    ToggleRotationScope,
    ToggleReadingMode,
    ToggleSidebar,
    ShowThumbnails,
    ShowOutline,
    ShowSearch,
    ShowRecent,
    ToggleTheme,
    ToggleFullscreen,
    Presentation,
    FocusSearch,
    FindNext,
    FindPrev,
    ToggleMatchCase,
    Copy,
    Highlight,
    Print,
    ExportRotated,
    OpenSettings,
    DocumentInfo,
}

/// Maps the current input snapshot to a command (if any).
///
/// Only key *presses* are considered; repeats are used for page navigation and
/// scrolling only, so holding an arrow key pages through the document.
pub fn command_for_input(input: &Input) -> Option<Command> {
    let ctrl = input.ctrl;
    let shift = input.shift;
    for event in &input.keys {
        let key = event.key;
        let repeat = event.repeat;
        let cmd = match key {
            Key::Char('o') | Key::Char('O') if ctrl => Some(Command::OpenDialog),
            Key::Char('w') if ctrl => Some(Command::CloseTab),
            Key::Char('q') if ctrl => Some(Command::Quit),
            Key::Char('g') if ctrl => Some(Command::GoToPageDialog),
            Key::Char('=') | Key::Char('+') if ctrl => Some(Command::ZoomIn),
            Key::Char('-') | Key::Char('_') if ctrl => Some(Command::ZoomOut),
            Key::Char('0') if ctrl => Some(Command::FitPage),
            Key::Char('1') if ctrl => Some(Command::FitWidth),
            Key::Char('2') if ctrl => Some(Command::ZoomActual),
            Key::Char('r') | Key::Char('R') if ctrl && shift => Some(Command::RotateCcw),
            Key::Char('r') | Key::Char('R') if ctrl && input.alt => Some(Command::Rotate180),
            Key::Char('r') | Key::Char('R') if ctrl => Some(Command::RotateCw),
            Key::Char('m') if ctrl => Some(Command::ToggleReadingMode),
            Key::Char('d') if ctrl => Some(Command::ToggleTheme),
            Key::Char('f') if ctrl => Some(Command::FocusSearch),
            Key::Char('c') if ctrl => Some(Command::Copy),
            Key::Char('h') if ctrl => Some(Command::Highlight),
            Key::Char('p') if ctrl => Some(Command::Print),
            Key::Char('e') if ctrl => Some(Command::ExportRotated),
            Key::Char(',') if ctrl => Some(Command::OpenSettings),
            Key::Char('i') if ctrl => Some(Command::DocumentInfo),
            Key::Tab if ctrl && shift => Some(Command::PrevDocument),
            Key::Tab if ctrl => Some(Command::NextDocument),
            Key::F3 if shift => Some(Command::FindPrev),
            Key::F3 => Some(Command::FindNext),
            Key::F5 => Some(Command::Presentation),
            Key::F9 => Some(Command::ToggleSidebar),
            Key::F11 => Some(Command::ToggleFullscreen),
            // Navigation (repeat allowed).
            Key::Right | Key::Down | Key::PageDown | Key::Space => Some(Command::NextPage),
            Key::Left | Key::Up | Key::PageUp => Some(Command::PrevPage),
            Key::Home if !ctrl => Some(Command::FirstPage),
            Key::End if !ctrl => Some(Command::LastPage),
            _ => None,
        };
        if cmd.is_some() && (repeat || !matches!(key, Key::Char(_)) || ctrl || input.alt) {
            return cmd;
        }
        if let Some(cmd) = cmd {
            // Plain characters without modifiers are text input, not commands.
            if !matches!(key, Key::Char(_)) {
                return Some(cmd);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;
    use crate::ui::KeyEvent;

    fn input_with(key: Key, ctrl: bool, shift: bool, alt: bool) -> Input {
        let mut input = Input {
            ctrl,
            shift,
            alt,
            mouse: Point::ZERO,
            ..Default::default()
        };
        input.keys.push(KeyEvent { key, repeat: false });
        input
    }

    #[test]
    fn ctrl_o_opens() {
        assert_eq!(
            command_for_input(&input_with(Key::Char('o'), true, false, false)),
            Some(Command::OpenDialog)
        );
    }

    #[test]
    fn arrows_navigate() {
        assert_eq!(
            command_for_input(&input_with(Key::Right, false, false, false)),
            Some(Command::NextPage)
        );
        assert_eq!(
            command_for_input(&input_with(Key::PageUp, false, false, false)),
            Some(Command::PrevPage)
        );
    }

    #[test]
    fn rotation_modifiers() {
        assert_eq!(
            command_for_input(&input_with(Key::Char('r'), true, false, false)),
            Some(Command::RotateCw)
        );
        assert_eq!(
            command_for_input(&input_with(Key::Char('R'), true, true, false)),
            Some(Command::RotateCcw)
        );
        assert_eq!(
            command_for_input(&input_with(Key::Char('r'), true, false, true)),
            Some(Command::Rotate180)
        );
    }

    #[test]
    fn zoom_keys() {
        assert_eq!(
            command_for_input(&input_with(Key::Char('+'), true, true, false)),
            Some(Command::ZoomIn)
        );
        assert_eq!(
            command_for_input(&input_with(Key::Char('-'), true, false, false)),
            Some(Command::ZoomOut)
        );
        assert_eq!(
            command_for_input(&input_with(Key::Char('0'), true, false, false)),
            Some(Command::FitPage)
        );
    }

    #[test]
    fn plain_letters_are_not_commands() {
        assert_eq!(command_for_input(&input_with(Key::Char('o'), false, false, false)), None);
        assert_eq!(command_for_input(&input_with(Key::Char('f'), false, false, false)), None);
    }

    #[test]
    fn function_keys() {
        assert_eq!(
            command_for_input(&input_with(Key::F11, false, false, false)),
            Some(Command::ToggleFullscreen)
        );
        assert_eq!(
            command_for_input(&input_with(Key::F3, false, true, false)),
            Some(Command::FindPrev)
        );
    }
}
