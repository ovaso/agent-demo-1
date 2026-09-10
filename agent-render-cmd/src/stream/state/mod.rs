//! Persistent block stack. Only the active leaf owns speculative text.
use crate::{Options, ansi::RESET};
use leaf::{Leaf, Paragraph};
use output::Output;
use stack::Stack;
use std::io::{self, Write};

mod accept;
mod emit;
mod leaf;
mod lex;
mod math;
mod output;
mod preview;
mod references;
mod stack;

/// Stateful streaming renderer: containers and the active block survive lines,
/// terminal scrollback and resize. Finished source lines are not retained by code.
pub struct Renderer<W> {
    options: Options,
    stack: Stack,
    output: Output<W>,
    raw: String,
    shown: usize,
    previewed_raw: usize,
    continued_line: bool,
    finished: bool,
    forcing: bool,
}

impl<W: Write> Renderer<W> {
    pub fn new(writer: W, mut options: Options) -> Self {
        if options.columns == 0 {
            options.columns = 80;
        }
        if options.rows == 0 {
            options.rows = 24;
        }
        Self {
            options: options.clone(),
            stack: Stack::default(),
            output: Output::new(writer, options),
            raw: String::new(),
            shown: 0,
            previewed_raw: 0,
            continued_line: false,
            finished: false,
            forcing: false,
        }
    }
    pub fn push(&mut self, delta: &str) -> io::Result<()> {
        if self.finished {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "renderer is already finished",
            ));
        }
        if !self.options.color {
            self.output.writer.write_all(delta.as_bytes())?;
            return self.output.writer.flush();
        }
        for part in delta.split_inclusive('\n') {
            let newline = part.ends_with('\n');
            let mut rest = part.strip_suffix('\n').unwrap_or(part);
            let limit = self.options.max_pending_bytes.clamp(4, 8 * 1024);
            while !rest.is_empty() {
                let room = limit.saturating_sub(self.raw.len());
                let mut end = room.min(rest.len());
                while !rest.is_char_boundary(end) {
                    end -= 1;
                }
                if end == 0 {
                    self.force_line_chunk()?;
                    continue;
                }
                self.raw.push_str(&rest[..end]);
                rest = &rest[end..];
            }
            if newline {
                let raw = std::mem::take(&mut self.raw);
                self.accept(&raw, true)?;
                self.raw = raw;
                self.raw.clear();
                self.shown = 0;
                self.previewed_raw = 0;
                self.continued_line = false;
            } else {
                self.preview_line()?;
            }
        }
        self.output.writer.flush()
    }
    pub fn resize(&mut self, columns: usize, rows: usize) -> io::Result<()> {
        let columns = if columns == 0 {
            self.options.columns
        } else {
            columns
        };
        let rows = if rows == 0 { self.options.rows } else { rows };
        if self.options.columns == columns && self.options.rows == rows {
            return Ok(());
        }
        if let Some(Leaf::Paragraph(p)) = self.stack.leaf()
            && !p.literal
        {
            let source = format!("{}\n{}", p.source, self.raw);
            let (_, unresolved) = self.output.references.render(&source, &self.options, &[]);
            self.output.deferred.frozen_unresolved |= unresolved;
        }
        let visible = self.output.resize()?;
        if visible {
            self.shown = self.previewed_raw;
            if let Some(mut leaf) = self.stack.take_leaf() {
                match leaf.as_mut() {
                    Leaf::Paragraph(p) => {
                        let separator = if p.source.is_empty() {
                            0
                        } else if p.after_blank {
                            2
                        } else {
                            1
                        };
                        p.shown = p.source.len()
                            + if self.raw.is_empty() {
                                0
                            } else {
                                separator + self.shown
                            };
                        p.literal = true;
                    }
                    Leaf::Math(math) => {
                        math.literal = true;
                        math.source.clear();
                        math.opener_written = true;
                    }
                    Leaf::Table(table) => {
                        table.rows.clear();
                        table.format = None;
                        table.header_written = false;
                        table.emitted = true;
                        self.shown = 0;
                    }
                    _ => {}
                }
                self.stack.put(leaf);
            }
            self.stack.mark_emitted();
        }
        if let Some(mut leaf) = self.stack.take_leaf() {
            if let Leaf::Table(table) = leaf.as_mut() {
                table.format = None;
                table.header_written = false;
            }
            self.stack.put(leaf);
        }
        self.options.columns = columns;
        self.options.rows = rows;
        self.output.options = self.options.clone();
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        if self.options.color {
            if !self.raw.is_empty() {
                let raw = std::mem::take(&mut self.raw);
                self.accept(&raw, false)?;
            }
            self.close_leaf()?;
            self.output.flush_pending()?;
            self.output.clear()?;
            self.output.writer.write_all(RESET.as_bytes())?;
        }
        self.output.writer.flush()?;
        self.stack.frames.clear();
        self.finished = true;
        Ok(())
    }
    pub fn into_inner(self) -> W {
        self.output.writer
    }
    fn force_line_chunk(&mut self) -> io::Result<()> {
        let raw = std::mem::take(&mut self.raw);
        if let Some(mut leaf) = self.stack.take_leaf() {
            match leaf.as_mut() {
                Leaf::Code(code) => code.painter.disable_highlighting(),
                Leaf::Paragraph(p) => p.literal = true,
                _ => {}
            }
            self.stack.put(leaf);
        }
        self.forcing = true;
        let result = self.accept(&raw, false);
        self.forcing = false;
        result?;
        if self.stack.leaf().is_none() {
            self.stack.put(Leaf::Paragraph(Paragraph {
                literal: true,
                ..Paragraph::default()
            }));
        }
        if matches!(self.stack.leaf(), Some(Leaf::Paragraph(_))) {
            self.flush_paragraph()?;
        }
        self.shown = 0;
        self.previewed_raw = 0;
        self.continued_line = true;
        Ok(())
    }
}
