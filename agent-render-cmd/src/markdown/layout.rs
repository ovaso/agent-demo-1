use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Options,
    ansi::{Line, Style},
    upstream::glamour::{Block, BlockStack},
};

struct Prefix {
    first: String,
    rest: String,
    used: bool,
}

pub(super) struct Layout<'a> {
    pub lines: Vec<Line>,
    options: &'a Options,
    stack: BlockStack,
    prefixes: Vec<Prefix>,
    margin: usize,
}

impl<'a> Layout<'a> {
    pub fn new(options: &'a Options, margin: usize) -> Self {
        let margin = margin.min(options.width().saturating_sub(2) / 2);
        let mut stack = BlockStack::default();
        stack.push(Block { indent: 0, margin });
        Self {
            lines: vec![Line::default()],
            options,
            stack,
            prefixes: Vec::new(),
            margin,
        }
    }

    pub fn available_width(&self) -> usize {
        self.stack.width(self.options.width()).max(1)
    }

    pub fn push_prefix(&mut self, first: String, rest: String, indent: usize) {
        self.finish_line();
        self.stack.push(Block { indent, margin: 0 });
        self.prefixes.push(Prefix {
            first,
            rest,
            used: false,
        });
    }

    pub fn pop_prefix(&mut self) {
        self.finish_line();
        self.prefixes.pop();
        self.stack.pop();
    }

    fn ensure_prefix(&mut self) {
        if self.lines.last().is_some_and(|line| !line.plain.is_empty()) {
            return;
        }
        let line = self.lines.last_mut().expect("layout always has a line");
        line.append(
            &" ".repeat(self.margin),
            Style::PLAIN,
            self.options.width_mode,
        );
        for prefix in &mut self.prefixes {
            let text = if prefix.used {
                &prefix.rest
            } else {
                &prefix.first
            };
            line.append(text, Style::PLAIN, self.options.width_mode);
            prefix.used = true;
        }
    }

    pub fn text(&mut self, text: &str, style: Style) {
        for grapheme in text.graphemes(true) {
            if matches!(grapheme, "\n" | "\r\n") {
                self.ensure_prefix();
                self.newline();
                continue;
            }
            let width = if grapheme == "\t" {
                4
            } else {
                self.options.width_mode.width(grapheme)
            };
            self.ensure_prefix();
            let line = self.lines.last().expect("layout always has a line");
            if line.width + width > self.options.width().saturating_sub(self.margin)
                && line.width > self.margin + self.stack.indent()
            {
                self.newline();
                self.ensure_prefix();
            }
            self.lines
                .last_mut()
                .expect("layout always has a line")
                .append(grapheme, style, self.options.width_mode);
        }
    }

    pub fn newline(&mut self) {
        self.lines.push(Line::default());
    }

    pub fn finish_line(&mut self) {
        if self.lines.last().is_some_and(|line| !line.plain.is_empty()) {
            self.newline();
        }
    }

    pub fn before_block(&mut self) {
        self.finish_line();
        if self.lines.len() > 1 && !self.lines[self.lines.len() - 2].plain.is_empty() {
            self.newline();
        }
    }

    pub fn block_lines(&mut self, lines: Vec<Line>) {
        self.finish_line();
        for line in lines {
            if line.width > self.available_width() {
                self.text(&line.plain, Style::PLAIN);
                self.finish_line();
                continue;
            }
            self.ensure_prefix();
            self.lines
                .last_mut()
                .expect("layout always has a line")
                .append_line(&line);
            self.newline();
        }
    }

    pub fn finish(mut self) -> Vec<Line> {
        while self.lines.last().is_some_and(|line| line.plain.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }
}
