use super::{super::frame::Frame, references::Deferred};
use crate::{
    Options,
    ansi::{Line, RESET},
    markdown::References,
};
use std::io::{self, Write};

pub(super) struct Output<W> {
    pub(super) writer: W,
    pub(super) options: Options,
    pub(super) references: References,
    pub(super) deferred: Deferred,
    preview: Option<Frame>,
    active_preview: bool,
    pub(super) wrote: bool,
    blank: bool,
}
impl<W: Write> Output<W> {
    pub(super) fn new(writer: W, options: Options) -> Self {
        Self {
            writer,
            options,
            references: References::default(),
            deferred: Deferred::default(),
            preview: None,
            active_preview: false,
            wrote: false,
            blank: true,
        }
    }
    pub(super) fn clear(&mut self) -> io::Result<()> {
        if let Some(frame) = self.preview.take() {
            frame.clear(&mut self.writer)?;
        }
        self.active_preview = false;
        Ok(())
    }
    fn paint(&mut self, lines: &[Line], active: bool) -> io::Result<bool> {
        let Some(frame) = Frame::measure(lines, &self.options) else {
            return Ok(false);
        };
        self.clear()?;
        if !lines.is_empty() {
            frame.paint(&mut self.writer, lines)?;
            self.preview = Some(frame);
            self.active_preview = active;
        }
        Ok(true)
    }
    pub(super) fn preview(&mut self, lines: &[Line]) -> io::Result<bool> {
        if !self.deferred.is_empty() {
            let mut combined = self.deferred.lines();
            combined.extend_from_slice(lines);
            if self.paint(&combined, !lines.is_empty())? {
                return Ok(true);
            }
            self.flush_pending()?;
        }
        self.paint(lines, !lines.is_empty())
    }
    pub(super) fn append(&mut self, lines: &[Line]) -> io::Result<()> {
        if !self.deferred.is_empty() {
            self.deferred.ready(lines);
            return self.paint_pending();
        }
        self.write_lines(lines)
    }
    pub(super) fn paint_pending(&mut self) -> io::Result<()> {
        if self.deferred.bytes() > self.options.max_pending_bytes {
            return self.flush_pending();
        }
        let lines = self.deferred.lines();
        if !self.paint(&lines, false)? {
            self.flush_pending()?;
        }
        Ok(())
    }
    pub(super) fn flush_pending(&mut self) -> io::Result<()> {
        if self.deferred.is_empty() {
            return Ok(());
        }
        self.deferred.frozen_unresolved |= self.deferred.unresolved();
        let pieces = std::mem::take(&mut self.deferred.pieces);
        for piece in pieces {
            self.write_lines(&piece.lines)?;
        }
        Ok(())
    }
    fn write_lines(&mut self, lines: &[Line]) -> io::Result<()> {
        self.clear()?;
        for line in lines {
            self.writer.write_all(line.ansi.as_bytes())?;
            self.writer.write_all(RESET.as_bytes())?;
            self.writer.write_all(b"\r\n")?;
            self.blank = line.plain.trim().is_empty() && !line.ansi.contains("[48;");
            self.wrote = true;
        }
        Ok(())
    }
    pub(super) fn gap(&mut self) -> io::Result<()> {
        if !self.deferred.is_empty() {
            if !self.deferred.ends_blank() {
                self.deferred.ready(&[Line::default()]);
            }
            return self.paint_pending();
        }
        self.clear()?;
        if self.wrote && !self.blank {
            self.writer.write_all(b"\r\n")?;
            self.blank = true;
        }
        Ok(())
    }
    pub(super) fn resize(&mut self) -> io::Result<bool> {
        // The terminal may already have reflowed these rows. Seal the visible
        // region without trying to erase or replay it at the old width.
        self.deferred.frozen_unresolved |= self.deferred.unresolved();
        self.deferred.pieces.clear();
        let active = self.active_preview;
        self.active_preview = false;
        if self.preview.take().is_some() {
            self.writer.write_all(RESET.as_bytes())?;
            self.writer.write_all(b"\r\n")?;
            self.wrote = true;
            self.blank = false;
        }
        Ok(active)
    }
}
