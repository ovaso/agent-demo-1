use std::{
    borrow::Cow,
    cell::Cell,
    env,
    io::{self, IsTerminal},
};

use rustyline::{
    Cmd, ColorMode, Config, Editor, GraphemeClusterMode, Helper, KeyCode, KeyEvent, Modifiers,
    completion::Completer,
    highlight::{CmdKind, Highlighter},
    hint::Hinter,
    history::DefaultHistory,
    validate::Validator,
};
use terminal_size::{Width, terminal_size_of};
use unicode_segmentation::UnicodeSegmentation;

pub(super) const USER_PROMPT: &str = "\n  ";
const BOX_STYLE: &str = "\x1b[48;5;236m\x1b[97m";
const CLEAR_ROW: &str = "\x1b[K";
const RESET: &str = "\x1b[0m";

pub(super) fn editor() -> rustyline::Result<Editor<TerminalStyle, DefaultHistory>> {
    let mut editor = Editor::new()?;
    editor.set_helper(Some(TerminalStyle::detect()));
    editor.bind_sequence(KeyEvent::ctrl('J'), Cmd::Newline);
    editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::ALT), Cmd::Newline);
    Ok(editor)
}

pub(super) fn styles_enabled() -> bool {
    let unsupported = env::var("TERM").is_ok_and(|term| {
        ["dumb", "cons25", "emacs"]
            .iter()
            .any(|name| term.eq_ignore_ascii_case(name))
    });
    io::stdout().is_terminal()
        && Config::default().color_mode() != ColorMode::Disabled
        && !unsupported
}

/// Paint the editable input only; the editor retains ownership of cursor layout.
pub(super) struct TerminalStyle {
    color: bool,
    columns: Cell<Option<usize>>,
    grapheme_mode: GraphemeClusterMode,
    tab_stop: usize,
}

impl TerminalStyle {
    pub(super) fn detect() -> Self {
        let config = Config::default();
        Self {
            color: styles_enabled(),
            columns: Cell::new(None),
            grapheme_mode: config.grapheme_cluster_mode(),
            tab_stop: usize::from(config.tab_stop()),
        }
    }
}

impl Highlighter for TerminalStyle {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        // Re-read dimensions on each repaint, including the editor's resize refresh.
        let columns = if self.color && default && prompt == USER_PROMPT {
            terminal_size_of(io::stdout())
                .map(|(Width(width), _)| usize::from(width))
                .filter(|width| *width > 2)
        } else {
            None
        };
        self.columns.set(columns);
        if columns.is_some() {
            Cow::Borrowed("\n\x1b[48;5;236m\x1b[97m\x1b[K  \x1b[0m")
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        match self.columns.get() {
            Some(columns) => Cow::Owned(paint_input(
                line,
                columns,
                self.grapheme_mode,
                self.tab_stop,
            )),
            None => Cow::Borrowed(line),
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        // Direct character insertion bypasses styling; request a repaint for edits.
        self.color
    }
}

fn paint_input(line: &str, columns: usize, mode: GraphemeClusterMode, tab_stop: usize) -> String {
    let mut output = String::with_capacity(line.len() + BOX_STYLE.len() + RESET.len());
    output.push_str(BOX_STYLE);
    let mut column = 2; // Visible spaces in USER_PROMPT.
    for grapheme in line.graphemes(true) {
        if grapheme == "\n" {
            output.push_str("\r\n");
            output.push_str(CLEAR_ROW);
            column = 0;
            continue;
        }
        let width = if grapheme == "\t" {
            tab_stop - column % tab_stop
        } else {
            usize::from(mode.width(grapheme))
        };
        if column + width > columns {
            // Materialize the same wrap the editor calculates, then fill the new row
            // before writing text. Erasing after text could erase the last character
            // of a completely full row while the terminal has a pending wrap.
            output.push_str("\r\n");
            output.push_str(CLEAR_ROW);
            column = 0;
        }
        if grapheme == "\t" {
            output.extend(std::iter::repeat_n(' ', width));
        } else {
            output.push_str(grapheme);
        }
        column += width;
    }
    output.push_str(RESET);
    output
}

impl Completer for TerminalStyle {
    type Candidate = String;
}

impl Hinter for TerminalStyle {
    type Hint = String;
}

impl Validator for TerminalStyle {}
impl Helper for TerminalStyle {}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(line: &str, columns: usize) -> String {
        paint_input(line, columns, GraphemeClusterMode::Unicode, 8)
    }

    #[test]
    fn empty_and_filled_input_restore_terminal_style() {
        assert_eq!(paint("", 20), format!("{BOX_STYLE}{RESET}"));
        assert_eq!(paint("你好", 20), format!("{BOX_STYLE}你好{RESET}"));
    }

    #[test]
    fn full_rows_keep_their_last_character() {
        assert_eq!(paint("123456", 8), format!("{BOX_STYLE}123456{RESET}"));
        assert_eq!(
            paint("1234567", 8),
            format!("{BOX_STYLE}123456\r\n{CLEAR_ROW}7{RESET}")
        );
    }

    #[test]
    fn chinese_and_combining_sequences_wrap_at_visible_width() {
        assert_eq!(
            paint("中文测试", 7),
            format!("{BOX_STYLE}中文\r\n{CLEAR_ROW}测试{RESET}")
        );
        assert_eq!(
            paint("e\u{301}中文", 6),
            format!("{BOX_STYLE}e\u{301}中\r\n{CLEAR_ROW}文{RESET}")
        );
    }

    #[test]
    fn pasted_lines_and_tabs_fill_each_row() {
        assert_eq!(
            paint("a\nb\t文", 12),
            format!("{BOX_STYLE}a\r\n{CLEAR_ROW}b       文{RESET}")
        );
    }

    #[test]
    fn narrower_terminal_recomputes_wrapping() {
        assert_eq!(paint("abcdef", 10), format!("{BOX_STYLE}abcdef{RESET}"));
        assert_eq!(
            paint("abcdef", 5),
            format!("{BOX_STYLE}abc\r\n{CLEAR_ROW}def{RESET}")
        );
    }

    #[test]
    fn plain_and_search_prompts_leave_input_unstyled() {
        let mut style = TerminalStyle::detect();
        style.color = false;
        assert_eq!(style.highlight_prompt(USER_PROMPT, true), USER_PROMPT);
        assert_eq!(style.highlight("中文", 0), "中文");
        assert!(!style.highlight_char("", 0, CmdKind::Other));
        style.color = true;
        style.columns.set(Some(80));
        assert_eq!(style.highlight_prompt("search: ", false), "search: ");
        assert_eq!(style.highlight("中文", 0), "中文");
    }
}
