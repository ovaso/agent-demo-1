//! A formula is a persistent leaf. Only its local source is editable; unfinished
//! input previews literally, so resize/overflow can seal it without losing text.
use super::{Renderer, leaf::Leaf};
use crate::{markdown, math};
use std::io::{self, Write};

#[derive(Clone, Copy)]
pub(super) enum Delimiter {
    Dollar,
    Bracket,
}
impl Delimiter {
    pub(super) fn opening(text: &str) -> Option<(Self, &str)> {
        let spaces = super::lex::leading_spaces(text);
        if spaces > 3 {
            return None;
        }
        let text = &text[spaces..];
        if let Some(rest) = text.strip_prefix("$$") {
            Some((Self::Dollar, rest))
        } else {
            text.strip_prefix(r"\[").map(|rest| (Self::Bracket, rest))
        }
    }
    fn open(self) -> &'static str {
        match self {
            Self::Dollar => "$$",
            Self::Bracket => r"\[",
        }
    }
    fn close(self) -> &'static str {
        match self {
            Self::Dollar => "$$",
            Self::Bracket => r"\]",
        }
    }
    fn closing(self, text: &str) -> Option<usize> {
        for (i, _) in text.match_indices(self.close()) {
            let escaped = text[..i].bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1;
            if !escaped {
                return Some(i);
            }
        }
        None
    }
}
pub(super) struct Math {
    delimiter: Delimiter,
    pub(super) source: String,
    pub(super) literal: bool,
    pub(super) opener_written: bool,
}
impl Math {
    pub(super) fn new(delimiter: Delimiter) -> Self {
        Self {
            delimiter,
            source: String::new(),
            literal: false,
            opener_written: false,
        }
    }
    fn original(&self, tail: &str, closed: bool) -> String {
        format!(
            "{}{}{tail}{}",
            if self.opener_written {
                ""
            } else {
                self.delimiter.open()
            },
            self.source,
            if closed { self.delimiter.close() } else { "" }
        )
    }
}
impl<W: Write> Renderer<W> {
    pub(super) fn accept_math(&mut self, text: &str, newline: bool) -> io::Result<()> {
        let Some(mut leaf) = self.stack.take_leaf() else {
            return Ok(());
        };
        let Leaf::Math(formula) = leaf.as_mut() else {
            unreachable!()
        };
        let close = if self.forcing {
            None
        } else {
            formula.delimiter.closing(text)
        };
        let body = close.map_or(text, |i| &text[..i]);
        if formula.literal {
            let raw = if formula.opener_written {
                text.to_owned()
            } else {
                format!("{}{text}", formula.delimiter.open())
            };
            self.literal_line(&raw)?;
            formula.opener_written = true;
        } else {
            formula.source.push_str(body);
            if close.is_none() && newline {
                formula.source.push('\n');
            }
            if close.is_some() {
                self.finish_math(formula, true)?;
            } else if formula.source.len() > self.options.max_pending_bytes.min(4096) {
                let raw = formula.original("", false);
                self.literal_line(raw.trim_end_matches('\n'))?;
                formula.source.clear();
                formula.literal = true;
                formula.opener_written = true;
            }
        }
        if let Some(close) = close {
            let tail = &text[close + formula.delimiter.close().len()..];
            self.stack.mark_emitted();
            if !tail.trim().is_empty() && !formula.literal {
                self.accept(tail, newline)?;
            }
        } else {
            self.stack.put(leaf);
            self.preview_math("")?;
        }
        Ok(())
    }
    pub(super) fn preview_math(&mut self, tail: &str) -> io::Result<()> {
        let Some(mut leaf) = self.stack.take_leaf() else {
            return Ok(());
        };
        let Leaf::Math(formula) = leaf.as_mut() else {
            unreachable!()
        };
        let raw = if formula.literal {
            tail[self.shown.min(tail.len())..].to_owned()
        } else {
            formula.original(tail, false)
        };
        self.previewed_raw = tail.len();
        let lines = markdown::scoped(
            raw.trim_end_matches('\n'),
            &self.options,
            &self.stack.prefixes(),
            true,
        );
        if raw.len() > self.options.max_pending_bytes.min(4096) || !self.output.preview(&lines)? {
            self.output.append(&lines)?;
            formula.literal = true;
            formula.opener_written = true;
            formula.source.clear();
            self.shown = tail.len();
            self.stack.mark_emitted();
        }
        self.stack.put(leaf);
        Ok(())
    }
    pub(super) fn finish_math(&mut self, formula: &mut Math, closed: bool) -> io::Result<()> {
        if formula.literal {
            return Ok(());
        }
        if closed {
            if let Some(lines) = math::display(
                formula.source.trim(),
                self.stack.width(&self.options),
                &self.options,
            ) {
                return self.output.append(&markdown::with_prefix(
                    lines,
                    &self.options,
                    &self.stack.prefixes(),
                ));
            }
            if let Some(text) = math::inline(formula.source.trim(), &self.options) {
                return self.output.append(&markdown::scoped(
                    &text,
                    &self.options,
                    &self.stack.prefixes(),
                    true,
                ));
            }
        }
        let raw = formula.original("", closed);
        self.output.append(&markdown::scoped(
            &raw,
            &self.options,
            &self.stack.prefixes(),
            true,
        ))
    }
}
