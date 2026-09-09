use crate::{code::StreamingCode, markdown::TableFormat};

pub(super) enum Leaf {
    Paragraph(Paragraph),
    Heading(String),
    Rule(String),
    Code(Code),
    Table(Table),
    Html(Html),
    Math(super::math::Math),
}

#[derive(Default)]
pub(super) struct Paragraph {
    pub(super) source: String,
    pub(super) after_blank: bool,
    pub(super) literal: bool,
    pub(super) shown: usize,
}

pub(super) enum CodeEnd {
    Fence {
        marker: u8,
        count: usize,
        indent: usize,
    },
    Indent,
}
pub(super) struct Code {
    pub(super) end: CodeEnd,
    pub(super) painter: StreamingCode,
    pub(super) lines: usize,
    pub(super) blank_lines: usize,
}

impl Code {
    pub(super) fn closing(&self, line: &str) -> bool {
        let CodeEnd::Fence { marker, count, .. } = self.end else {
            return false;
        };
        let spaces = super::lex::leading_spaces(line);
        if spaces > 3 {
            return false;
        }
        let line = &line[spaces..];
        let markers = line.bytes().take_while(|b| b == &marker).count();
        markers >= count && line[markers..].trim().is_empty()
    }
    pub(super) fn content<'a>(&self, line: &'a str) -> Option<&'a str> {
        let spaces = super::lex::leading_spaces(line);
        match self.end {
            CodeEnd::Fence { indent, .. } => Some(&line[spaces.min(indent)..]),
            CodeEnd::Indent if line.trim().is_empty() => Some(""),
            CodeEnd::Indent if spaces >= 4 => Some(&line[4..]),
            CodeEnd::Indent => None,
        }
    }
}

pub(super) struct Table {
    pub(super) header: String,
    pub(super) separator: String,
    pub(super) rows: Vec<String>,
    pub(super) format: Option<TableFormat>,
    pub(super) header_written: bool,
    pub(super) emitted: bool,
}
impl Table {
    pub(super) fn source(&self, next: Option<&str>) -> String {
        let mut source = format!("{}\n{}\n", self.header, self.separator);
        for row in &self.rows {
            source.push_str(row);
            source.push('\n');
        }
        if let Some(row) = next {
            source.push_str(row);
            source.push('\n');
        }
        source
    }
}

pub(super) struct Html {
    end: HtmlEnd,
    tail: String,
    seen_end: bool,
    blank: bool,
}
enum HtmlEnd {
    Blank,
    Token(String),
}
impl Html {
    pub(super) fn new(line: &str) -> Self {
        let text = line.trim_start().to_ascii_lowercase();
        let token = if text.starts_with("<!--") {
            Some("-->".into())
        } else if text.starts_with("<?") {
            Some("?>".into())
        } else if text.starts_with("<![cdata[") {
            Some("]]>".into())
        } else if text.starts_with("<!") {
            Some(">".into())
        } else {
            ["script", "pre", "style", "textarea"]
                .into_iter()
                .find(|tag| text.starts_with(&format!("<{tag}")))
                .map(|tag| format!("</{tag}>"))
        };
        Self {
            end: token.map_or(HtmlEnd::Blank, HtmlEnd::Token),
            tail: String::new(),
            seen_end: false,
            blank: true,
        }
    }
    pub(super) fn closes(&mut self, line: &str, complete: bool) -> bool {
        self.blank &= line.trim().is_empty();
        if let HtmlEnd::Token(token) = &self.end {
            let text = format!("{}{}", self.tail, line.to_ascii_lowercase());
            self.seen_end |= text.contains(token);
            let mut start = text.len().saturating_sub(token.len());
            while !text.is_char_boundary(start) {
                start += 1;
            }
            self.tail = text[start..].to_owned();
        }
        if !complete {
            return false;
        }
        let closed = match &self.end {
            HtmlEnd::Blank => self.blank,
            HtmlEnd::Token(_) => self.seen_end,
        };
        self.blank = true;
        self.tail.clear();
        closed
    }
}
