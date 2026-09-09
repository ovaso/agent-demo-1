use pulldown_cmark::{CodeBlockKind, Event, Tag};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListKind {
    Bullet(u8),
    Ordered(u8),
}

#[derive(Clone, Debug)]
pub(super) struct Marker {
    pub(super) kind: ListKind,
    pub(super) number: u64,
    pub(super) consumed: usize,
    pub(super) indent: usize,
}

pub(super) fn leading_spaces(text: &str) -> usize {
    text.bytes().take_while(|b| *b == b' ').count()
}

pub(super) fn marker(text: &str) -> Option<Marker> {
    let gap = leading_spaces(text);
    if gap > 3 {
        return None;
    }
    let rest = &text[gap..];
    let first = *rest.as_bytes().first()?;
    let (kind, number, end) = if matches!(first, b'-' | b'+' | b'*') {
        (ListKind::Bullet(first), 0, 1)
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let delimiter = *rest.as_bytes().get(digits)?;
        if !matches!(delimiter, b'.' | b')') {
            return None;
        }
        (
            ListKind::Ordered(delimiter),
            rest[..digits].parse().ok()?,
            digits + 1,
        )
    };
    if rest.as_bytes().get(end).is_some_and(|b| *b != b' ') {
        return None;
    }
    let spaces = leading_spaces(&rest[end..]);
    let padding = if spaces == 0 || spaces > 4 { 1 } else { spaces };
    let consumed = (gap + end + padding).min(text.len());
    Some(Marker {
        kind,
        number,
        consumed,
        indent: gap + end + padding,
    })
}

pub(super) fn quote(text: &str) -> Option<usize> {
    let spaces = leading_spaces(text);
    if spaces <= 3 && text.as_bytes().get(spaces) == Some(&b'>') {
        Some(spaces + 1 + usize::from(text.as_bytes().get(spaces + 1) == Some(&b' ')))
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub(super) enum Kind {
    Paragraph,
    Heading,
    Rule,
    Fenced {
        marker: u8,
        count: usize,
        indent: usize,
        info: String,
    },
    Indented,
    Html,
    Math,
}

pub(super) fn kind(text: &str) -> Kind {
    if super::math::Delimiter::opening(text).is_some() {
        return Kind::Math;
    }
    for event in crate::markdown::parser(text) {
        match event {
            Event::Start(Tag::Heading { .. }) => return Kind::Heading,
            Event::Rule => return Kind::Rule,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let indent = leading_spaces(text);
                let trimmed = &text[indent..];
                let marker = trimmed.as_bytes().first().copied().unwrap_or(b'`');
                let count = trimmed.bytes().take_while(|b| *b == marker).count();
                return Kind::Fenced {
                    marker,
                    count,
                    indent,
                    info: info.to_string(),
                };
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => return Kind::Indented,
            Event::Start(Tag::HtmlBlock) => return Kind::Html,
            Event::Start(Tag::Paragraph) => return Kind::Paragraph,
            _ => {}
        }
    }
    Kind::Paragraph
}

pub(super) fn interrupts(text: &str) -> bool {
    if quote(text).is_some() {
        return true;
    }
    if let Some(marker) = marker(text) {
        return !matches!(marker.kind, ListKind::Ordered(_)) || marker.number == 1;
    }
    matches!(
        kind(text),
        Kind::Heading | Kind::Rule | Kind::Fenced { .. } | Kind::Html | Kind::Math
    )
}

pub(super) fn table_header(header: &str, next: &str) -> bool {
    if !next.contains('-')
        || !next
            .bytes()
            .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
    {
        return false;
    }
    let source = format!("{header}\n{next}\n");
    matches!(
        crate::markdown::parser(&source).next(),
        Some(Event::Start(Tag::Table(_)))
    )
}

pub(super) fn setext(source: &str, next: &str) -> bool {
    let indent = leading_spaces(next);
    let underline = next[indent..].trim_end();
    if indent > 3
        || underline.is_empty()
        || !matches!(underline.as_bytes()[0], b'=' | b'-')
        || !underline.bytes().all(|b| b == underline.as_bytes()[0])
    {
        return false;
    }
    let source = format!("{source}\n{next}\n");
    matches!(
        crate::markdown::parser(&source).next(),
        Some(Event::Start(Tag::Heading { .. }))
    )
}

/// Do not publish an ambiguous new block's prefix as paragraph text. Its full
/// logical line will select the stack transition; ordinary text still previews.
pub(super) fn could_start_block(text: &str) -> bool {
    let text = text.trim_start_matches(' ');
    if text.is_empty() {
        return false;
    }
    if quote(text).is_some() || marker(text).is_some() {
        return true;
    }
    if text.len() <= 9 && text.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if text.starts_with('|') {
        return true;
    }
    if text
        .bytes()
        .all(|b| matches!(b, b'*' | b'-' | b'_' | b'`' | b'~' | b'#' | b' '))
    {
        return true;
    }
    !matches!(kind(text), Kind::Paragraph)
}

pub(super) fn expand_tabs(text: &str, mode: crate::WidthMode) -> std::borrow::Cow<'_, str> {
    use unicode_segmentation::UnicodeSegmentation;
    if !text.contains('\t') {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut expanded = String::with_capacity(text.len());
    let mut column = 0;
    for grapheme in text.graphemes(true) {
        if grapheme == "\t" {
            let count = 4 - column % 4;
            expanded.extend(std::iter::repeat_n(' ', count));
            column += count;
        } else {
            expanded.push_str(grapheme);
            column += mode.width(grapheme);
        }
    }
    std::borrow::Cow::Owned(expanded)
}
