use super::{
    Renderer,
    leaf::{Code, Leaf},
};
use crate::markdown;
use std::io::{self, Write};

impl<W: Write> Renderer<W> {
    pub(super) fn code_header(&mut self, code: &Code) -> io::Result<()> {
        let line = code
            .painter
            .header(self.stack.width(&self.options), &self.options);
        let line = self.stack.prefix_line(line, &self.options);
        self.output.append(&[line])?;
        self.stack.mark_emitted();
        Ok(())
    }
    pub(super) fn open_code(&mut self, code: Code) -> io::Result<()> {
        self.code_header(&code)?;
        self.stack.put(Leaf::Code(code));
        Ok(())
    }
    pub(super) fn code_line(
        &mut self,
        code: &mut Code,
        text: &str,
        newline: bool,
    ) -> io::Result<()> {
        let source = if newline {
            format!("{text}\n")
        } else {
            text.to_owned()
        };
        let lines = code.painter.line_from(
            &source,
            true,
            self.shown.min(text.len()),
            self.stack.width(&self.options),
            &self.options,
        );
        if self.shown == 0 || self.shown < text.len() {
            let lines = markdown::with_prefix(lines, &self.options, &self.stack.prefixes());
            self.output.append(&lines)?;
            self.stack.mark_emitted();
        } else {
            self.output.clear()?;
        }
        code.lines += 1;
        Ok(())
    }
    pub(super) fn literal_line(&mut self, text: &str) -> io::Result<()> {
        let text = &text[self.shown.min(text.len())..];
        let lines = markdown::scoped(text, &self.options, &self.stack.prefixes(), true);
        self.output.append(&lines)?;
        self.stack.mark_emitted();
        Ok(())
    }
    pub(super) fn close_leaf(&mut self) -> io::Result<()> {
        let Some(mut leaf) = self.stack.take_leaf() else {
            return Ok(());
        };
        match leaf.as_mut() {
            Leaf::Paragraph(p) => {
                self.output.append_markdown(
                    &p.source[p.shown.min(p.source.len())..],
                    &self.stack.prefixes(),
                    p.literal || p.shown > 0,
                )?;
            }
            Leaf::Heading(source) | Leaf::Rule(source) => {
                self.output
                    .append_markdown(source, &self.stack.prefixes(), false)?;
            }
            Leaf::Code(code) => {
                if code.lines == 0 {
                    self.code_line(code, "", true)?;
                }
                let footer = code
                    .painter
                    .footer(self.stack.width(&self.options), &self.options);
                self.output
                    .append(&[self.stack.prefix_line(footer, &self.options)])?;
            }
            Leaf::Table(_) => {
                self.stack.put(leaf);
                self.refresh_table(true)?;
                self.stack.take_leaf();
            }
            Leaf::Math(math) => self.finish_math(math, false)?,
            Leaf::Html(_) => {}
        }
        self.stack.mark_emitted();
        Ok(())
    }
    pub(super) fn flush_paragraph(&mut self) -> io::Result<()> {
        let Some(mut leaf) = self.stack.take_leaf() else {
            return Ok(());
        };
        if let Leaf::Paragraph(p) = leaf.as_mut() {
            let lines = self.output.render_markdown(
                &p.source[p.shown.min(p.source.len())..],
                &self.stack.prefixes(),
                p.literal || p.shown > 0,
            );
            self.output.append(&lines)?;
            p.source.clear();
            p.shown = 0;
            p.literal = true;
            self.stack.mark_emitted();
        }
        self.stack.put(leaf);
        Ok(())
    }
    pub(super) fn refresh_table(&mut self, force: bool) -> io::Result<()> {
        let Some(mut leaf) = self.stack.take_leaf() else {
            return Ok(());
        };
        if let Leaf::Table(table) = leaf.as_mut() {
            if table.rows.is_empty() && (table.header_written || (force && table.emitted)) {
                self.stack.put(leaf);
                return Ok(());
            }
            let source = table.source(None);
            let source = self.output.references.source(&source);
            if let Some((format, raw)) = markdown::format_table(
                &source,
                self.stack.width(&self.options),
                self.options.width_mode,
                table.format.as_ref(),
                !table.header_written,
                &self.options,
            ) {
                let lines = markdown::with_prefix(raw, &self.options, &self.stack.prefixes());
                if force
                    || table.header_written
                    || table.rows.len() >= 8
                    || source.len() >= self.options.max_pending_bytes
                    || !self.output.preview(&lines)?
                {
                    self.output.append(&lines)?;
                    self.stack.mark_emitted();
                    table.rows.clear();
                    table.format = Some(format);
                    table.header_written = true;
                    table.emitted = true;
                }
            }
        }
        self.stack.put(leaf);
        Ok(())
    }
}
