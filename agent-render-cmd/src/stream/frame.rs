use std::io::{self, Write};

use crate::{
    Options,
    ansi::{Line, RESET},
};

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub rows: usize,
    wrapped: bool,
}

impl Frame {
    pub fn measure(lines: &[Line], options: &Options) -> Option<Self> {
        if lines.iter().any(|line| line.width > options.columns) {
            return None;
        }
        let wrapped = lines
            .last()
            .is_some_and(|line| line.width == options.columns);
        let rows = lines.len().saturating_sub(1) + usize::from(wrapped);
        (options.columns >= 4 && rows < options.preview_rows()).then_some(Self { rows, wrapped })
    }

    pub fn paint(self, writer: &mut impl Write, lines: &[Line]) -> io::Result<()> {
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                writer.write_all(b"\r\n")?;
            }
            writer.write_all(line.ansi.as_bytes())?;
            writer.write_all(RESET.as_bytes())?;
        }
        if self.wrapped {
            writer.write_all(b"\r\n")?;
        }
        Ok(())
    }

    pub fn clear(self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(b"\r")?;
        for _ in 0..self.rows {
            writer.write_all(b"\x1b[2K\x1b[A")?;
        }
        writer.write_all(b"\x1b[2K\r")
    }
}
