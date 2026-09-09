use super::{Renderer, leaf::Leaf};
use crate::markdown;
use std::io::{self, Write};

impl<W: Write> Renderer<W> {
    pub(super) fn preview_paragraph(&mut self, next: Option<&str>) -> io::Result<()> {
        let Some(Leaf::Paragraph(p)) = self.stack.leaf() else {
            return Ok(());
        };
        let source = if let Some(next) = next {
            format!(
                "{}{}{}",
                p.source,
                if p.source.is_empty() {
                    ""
                } else if p.after_blank {
                    "\n\n"
                } else {
                    "\n"
                },
                next
            )
        } else {
            p.source.clone()
        };
        let visible = &source[p.shown.min(source.len())..];
        let lines =
            self.output
                .render_markdown(visible, &self.stack.prefixes(), p.literal || p.shown > 0);
        if source.len() > self.options.max_pending_bytes || !self.output.preview(&lines)? {
            // The paragraph context remains on the stack even when speculative
            // inline formatting must be fixed to keep memory/scrollback bounded.
            self.output.append(&lines)?;
            if let Some(mut leaf) = self.stack.take_leaf() {
                if let Leaf::Paragraph(p) = leaf.as_mut() {
                    p.source.clear();
                    p.shown = next.map_or(0, |text| text.len());
                    p.literal = true;
                }
                self.stack.put(leaf);
            }
            self.stack.mark_emitted();
            if let Some(next) = next {
                self.shown += next.len();
            }
        }
        if let Some(next) = next {
            self.previewed_raw = next.len();
        }
        Ok(())
    }
    pub(super) fn preview_line(&mut self) -> io::Result<()> {
        let buffer = std::mem::take(&mut self.raw);
        let result = self.preview_source(&buffer);
        self.raw = buffer;
        result
    }
    fn preview_source(&mut self, source: &str) -> io::Result<()> {
        let raw = super::lex::expand_tabs(source, self.options.width_mode);
        let lazy = matches!(self.stack.leaf(), Some(Leaf::Paragraph(p)) if !p.after_blank);
        let (matched, rest) = if self.continued_line {
            (self.stack.frames.len(), raw.as_ref())
        } else {
            self.stack.continuation(&raw, lazy)
        };
        if matched < self.stack.frames.len() {
            return Ok(());
        }
        if !matches!(
            self.stack.leaf(),
            Some(Leaf::Code(_) | Leaf::Html(_) | Leaf::Table(_) | Leaf::Math(_))
        ) && super::lex::could_start_block(rest)
        {
            return Ok(());
        }
        let mut leaf = self.stack.take_leaf();
        match leaf.as_deref_mut() {
            Some(Leaf::Math(_)) => {
                self.stack.put(leaf.take().unwrap());
                return self.preview_math(rest);
            }
            Some(Leaf::Code(code)) => {
                if let Some(content) = if self.continued_line {
                    Some(rest)
                } else {
                    code.content(rest)
                } {
                    self.previewed_raw = content.len();
                    let lines = code.painter.line_from(
                        content,
                        false,
                        self.shown.min(content.len()),
                        self.stack.width(&self.options),
                        &self.options,
                    );
                    let lines = markdown::with_prefix(lines, &self.options, &self.stack.prefixes());
                    if !self.output.preview(&lines)? {
                        // Freeze the visible portion; keep the fence and language
                        // state. The next fragment continues this code block.
                        self.output.append(&lines)?;
                        self.shown = content.len();
                        self.stack.mark_emitted();
                    }
                }
            }
            Some(Leaf::Paragraph(_)) => {
                self.stack.put(leaf.take().unwrap());
                return self.preview_paragraph(Some(rest));
            }
            Some(Leaf::Table(_)) => {} // A partial row cannot determine cell boundaries.
            Some(Leaf::Html(_)) => {
                self.previewed_raw = rest.len();
                let lines = markdown::scoped(
                    &rest[self.shown.min(rest.len())..],
                    &self.options,
                    &self.stack.prefixes(),
                    true,
                );
                if !self.output.preview(&lines)? {
                    self.output.append(&lines)?;
                    self.shown = rest.len();
                    self.stack.mark_emitted();
                }
            }
            _ => {
                if !self.continued_line
                    && super::lex::quote(rest).is_none()
                    && super::lex::marker(rest).is_none()
                    && matches!(super::lex::kind(rest), super::lex::Kind::Paragraph)
                {
                    if self.stack.is_root() {
                        self.output.gap()?;
                    }
                    self.stack
                        .put(Leaf::Paragraph(super::leaf::Paragraph::default()));
                    return self.preview_paragraph(Some(rest));
                }
                if !self.continued_line {
                    return Ok(());
                }
                let lines = markdown::scoped(
                    rest,
                    &self.options,
                    &self.stack.prefixes(),
                    self.continued_line,
                );
                if !self.output.preview(&lines)? {
                    self.output.append(&lines)?;
                    self.shown = rest.len();
                    leaf = Some(Box::new(Leaf::Paragraph(super::leaf::Paragraph {
                        literal: true,
                        shown: rest.len(),
                        ..Default::default()
                    })));
                    self.stack.mark_emitted();
                }
            }
        }
        if let Some(leaf) = leaf {
            self.stack.put(leaf);
        }
        Ok(())
    }
}
