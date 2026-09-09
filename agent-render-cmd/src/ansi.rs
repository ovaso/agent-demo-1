use std::fmt::Write as _;

use crate::WidthMode;

pub(crate) const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Color {
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    fn write(self, output: &mut String, background: bool) {
        let prefix = if background { 48 } else { 38 };
        match self {
            Self::Indexed(index) => {
                let _ = write!(output, "\x1b[{prefix};5;{index}m");
            }
            Self::Rgb(r, g, b) => {
                let _ = write!(output, "\x1b[{prefix};2;{r};{g};{b}m");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
}

impl Style {
    pub const PLAIN: Self = Self {
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        fg: None,
        bg: None,
    };

    pub fn start(self, output: &mut String) {
        output.push_str(RESET);
        if self.bold {
            output.push_str("\x1b[1m");
        }
        if self.italic {
            output.push_str("\x1b[3m");
        }
        if self.underline {
            output.push_str("\x1b[4m");
        }
        if self.strike {
            output.push_str("\x1b[9m");
        }
        if let Some(color) = self.fg {
            color.write(output, false);
        }
        if let Some(color) = self.bg {
            color.write(output, true);
        }
    }
}

#[derive(Default, Clone)]
pub(crate) struct Line {
    pub plain: String,
    pub ansi: String,
    pub width: usize,
    last_style: Option<Style>,
}

impl Line {
    pub fn append(&mut self, text: &str, style: Style, mode: WidthMode) {
        use unicode_segmentation::UnicodeSegmentation;
        if self.last_style != Some(style) {
            style.start(&mut self.ansi);
            self.last_style = Some(style);
        }
        for ch in text.chars() {
            let replacement = match ch {
                '\t' => "    ",
                c if c.is_control() => "�",
                _ => {
                    self.plain.push(ch);
                    self.ansi.push(ch);
                    continue;
                }
            };
            self.plain.push_str(replacement);
            self.ansi.push_str(replacement);
        }
        // Count this text with the same normalization used for terminal output.
        self.width += text
            .graphemes(true)
            .map(|g| {
                if g == "\t" {
                    4
                } else if g.chars().any(char::is_control) {
                    g.chars().count()
                } else {
                    mode.width(g)
                }
            })
            .sum::<usize>();
    }

    pub fn append_line(&mut self, other: &Self) {
        self.plain.push_str(&other.plain);
        self.ansi.push_str(&other.ansi);
        self.width += other.width;
        self.last_style = other.last_style;
    }
}
