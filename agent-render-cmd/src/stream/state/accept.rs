use super::{
    Renderer,
    leaf::{Code, CodeEnd, Html, Leaf, Paragraph, Table},
    lex::{self, Kind},
    stack::Frame,
};
use crate::{code::StreamingCode, markdown};
use std::io::{self, Write};

impl<W: Write> Renderer<W> {
    pub(super) fn accept(&mut self, raw: &str, newline: bool) -> io::Result<()> {
        let expanded = lex::expand_tabs(raw.trim_end_matches('\r'), self.options.width_mode);
        let lazy = matches!(self.stack.leaf(), Some(Leaf::Paragraph(p)) if !p.after_blank);
        let mut current = self.stack.take_leaf();
        let (matched, rest) = if self.continued_line {
            (self.stack.frames.len(), expanded.as_ref())
        } else {
            self.stack.continuation(&expanded, lazy)
        };
        if matched < self.stack.frames.len() {
            if let Some(leaf) = current.take() {
                self.stack.put(leaf);
                self.close_leaf()?;
            }
            self.stack.truncate(matched);
        }
        let mut text = rest;
        if let Some(mut leaf) = current.take() {
            match leaf.as_mut() {
                Leaf::Math(_) => {
                    self.stack.put(leaf);
                    return self.accept_math(text, newline);
                }
                Leaf::Code(code) => {
                    if !self.continued_line && !self.forcing && code.closing(text) {
                        self.stack.put(leaf);
                        return self.close_leaf();
                    }
                    if self.continued_line || code.content(text).is_some() {
                        let content = if self.continued_line {
                            text
                        } else {
                            code.content(text).unwrap()
                        };
                        if matches!(code.end, CodeEnd::Indent) && content.is_empty() {
                            code.blank_lines += 1;
                        } else {
                            while code.blank_lines > 0 {
                                self.code_line(code, "", true)?;
                                code.blank_lines -= 1;
                            }
                            self.code_line(code, content, newline)?;
                        }
                        self.stack.put(leaf);
                        return Ok(());
                    }
                    self.stack.put(leaf);
                    self.close_leaf()?;
                }
                Leaf::Html(html) => {
                    let closed = html.closes(text, !self.forcing);
                    if !text.is_empty() {
                        self.literal_line(text)?;
                    }
                    if !closed {
                        self.stack.put(leaf);
                    }
                    return Ok(());
                }
                Leaf::Table(table) => {
                    if self.forcing || self.continued_line {
                        self.stack.put(leaf);
                        self.refresh_table(true)?;
                        self.literal_line(text)?;
                        return Ok(());
                    }
                    let probe = format!("{}\n{}\n{text}\n", table.header, table.separator);
                    if !text.trim().is_empty() && markdown::is_table_row(&probe) {
                        table.rows.push(text.into());
                        self.stack.put(leaf);
                        return self.refresh_table(false);
                    }
                    self.stack.put(leaf);
                    self.close_leaf()?;
                }
                Leaf::Paragraph(p) => {
                    if text.trim().is_empty() {
                        p.after_blank = true;
                        self.stack.put(leaf);
                        return Ok(());
                    }
                    if p.after_blank && text.starts_with('[') && text.contains("]:") {
                        p.source.push_str("\n\n");
                        p.source.push_str(text);
                        self.stack.put(leaf);
                        return self.preview_paragraph(None);
                    }
                    if !p.after_blank
                        && !p.literal
                        && lex::table_header(p.source.rsplit('\n').next().unwrap_or(""), text)
                    {
                        let start = p.source.rfind('\n').map_or(0, |n| n + 1);
                        let header = p.source[start..].to_owned();
                        if start > p.shown {
                            let lines = markdown::scoped(
                                &p.source[p.shown..start],
                                &self.options,
                                &self.stack.prefixes(),
                                false,
                            );
                            self.output.append(&lines)?;
                            self.stack.mark_emitted();
                        }
                        self.stack.put(Leaf::Table(Table {
                            header,
                            separator: text.into(),
                            rows: Vec::new(),
                            format: None,
                            header_written: false,
                            emitted: false,
                        }));
                        return self.refresh_table(false);
                    }
                    if !p.after_blank && !p.literal && lex::setext(&p.source, text) {
                        let source = format!("{}\n{text}\n", p.source);
                        self.stack.put(Leaf::Heading(source));
                        return self.close_leaf();
                    }
                    if !p.after_blank && (!lex::interrupts(text) || self.continued_line) {
                        if !p.source.is_empty() {
                            p.source.push('\n');
                        }
                        if p.source.is_empty() {
                            p.shown = p.shown.max(self.shown);
                        }
                        p.source.push_str(text);
                        self.stack.put(leaf);
                        return self.preview_paragraph(None);
                    }
                    self.stack.put(leaf);
                    self.close_leaf()?;
                }
                _ => {
                    self.stack.put(leaf);
                    self.close_leaf()?;
                }
            }
        }
        if text.trim().is_empty() {
            if self.stack.frames.iter().any(|f| matches!(f, Frame::Quote)) {
                let line = self.stack.blank(&self.options);
                self.output.append(&[line])?;
            } else {
                self.output.gap()?;
            }
            return Ok(());
        }
        if self.stack.is_root() {
            self.output.gap()?;
        }
        loop {
            if matches!(lex::kind(text), Kind::Rule | Kind::Indented) {
                break;
            }
            if let Some(n) = lex::quote(text) {
                self.stack.frames.push(Frame::Quote);
                text = &text[n..];
            } else if let Some(marker) = lex::marker(text) {
                let n = marker.consumed;
                self.stack.new_item(marker);
                text = &text[n..];
            } else {
                break;
            }
        }
        self.stack.close_unattached_list();
        if self.stack.is_root() {
            self.output.gap()?;
        }
        let kind = lex::kind(text);
        let kind = if self.forcing && !matches!(kind, Kind::Indented) {
            Kind::Paragraph
        } else {
            kind
        };
        match kind {
            Kind::Math => {
                let (delimiter, content) =
                    super::math::Delimiter::opening(text).expect("math opener");
                self.stack
                    .put(Leaf::Math(super::math::Math::new(delimiter)));
                self.accept_math(content, newline)
            }
            Kind::Heading => {
                self.stack.put(Leaf::Heading(text.into()));
                self.close_leaf()
            }
            Kind::Rule => {
                self.stack.put(Leaf::Rule(text.into()));
                self.close_leaf()
            }
            Kind::Fenced {
                marker,
                count,
                indent,
                info,
            } => {
                let code = Code {
                    end: CodeEnd::Fence {
                        marker,
                        count,
                        indent,
                    },
                    painter: StreamingCode::new(&info),
                    lines: 0,
                    blank_lines: 0,
                };
                self.open_code(code)
            }
            Kind::Indented => {
                let mut code = Code {
                    end: CodeEnd::Indent,
                    painter: StreamingCode::new("text"),
                    lines: 0,
                    blank_lines: 0,
                };
                self.code_header(&code)?;
                self.code_line(&mut code, &text[4..], newline)?;
                self.stack.put(Leaf::Code(code));
                Ok(())
            }
            Kind::Html => {
                let mut html = Html::new(text);
                let closed = html.closes(text, !self.forcing);
                self.literal_line(text)?;
                if !closed {
                    self.stack.put(Leaf::Html(html));
                }
                Ok(())
            }
            Kind::Paragraph => {
                let task = matches!(
                    self.stack.frames.last(),
                    Some(Frame::Item { emitted: false, .. })
                );
                let source = if task && (text.starts_with("[x] ") || text.starts_with("[X] ")) {
                    format!("[✓] {}", &text[4..])
                } else {
                    text.to_owned()
                };
                if source.is_empty() {
                    let line = self.stack.blank(&self.options);
                    self.output.append(&[line])?;
                    self.stack.mark_emitted();
                    return Ok(());
                }
                self.stack.put(Leaf::Paragraph(Paragraph {
                    source,
                    literal: self.continued_line || self.forcing || self.shown > 0,
                    shown: self.shown.min(text.len()),
                    after_blank: false,
                }));
                self.preview_paragraph(None)
            }
        }
    }
}
