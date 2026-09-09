use unicode_segmentation::UnicodeSegmentation;

use super::highlight::Span;
use crate::{
    WidthMode,
    ansi::{Color, Line, Style},
};

pub(super) const BODY: Style = Style {
    fg: Some(Color::Rgb(216, 222, 233)),
    bg: Some(Color::Rgb(35, 40, 48)),
    ..Style::PLAIN
};
const HEADER: Style = Style {
    fg: Some(Color::Rgb(180, 190, 205)),
    bg: Some(Color::Rgb(52, 61, 70)),
    bold: true,
    ..Style::PLAIN
};

pub(super) fn render<'a>(
    label: &str,
    source: &str,
    spans: impl Iterator<Item = &'a Span>,
    width: usize,
    mode: WidthMode,
) -> Vec<Line> {
    render_from(label, source, spans, 0, width, mode)
}

pub(super) fn render_from<'a>(
    label: &str,
    source: &str,
    spans: impl Iterator<Item = &'a Span>,
    from: usize,
    width: usize,
    mode: WidthMode,
) -> Vec<Line> {
    let mut output = vec![header(label, width, mode)];
    output.extend(body(source, spans, from, width, mode));
    output.push(footer(width, mode));
    output
}

fn dimensions(width: usize) -> (usize, usize, usize, String) {
    let width = width.max(1);
    let padding = if width >= 8 {
        2
    } else if width >= 4 {
        1
    } else {
        0
    };
    (
        width,
        padding,
        width.saturating_sub(2 * padding).max(1),
        " ".repeat(width),
    )
}

pub(super) fn header(label: &str, width: usize, mode: WidthMode) -> Line {
    let (width, padding, content_width, spaces) = dimensions(width);
    let mut line = Line::default();
    line.append(&spaces[..padding], HEADER, mode);
    let mut used = 0;
    for grapheme in label.graphemes(true) {
        let size = mode.width(grapheme);
        if used + size > content_width {
            break;
        }
        line.append(grapheme, HEADER, mode);
        used += size;
    }
    fill(&mut line, width, HEADER, mode, &spaces);
    line
}

pub(super) fn footer(width: usize, mode: WidthMode) -> Line {
    let mut line = Line::default();
    line.append(&" ".repeat(width.max(1)), BODY, mode);
    line
}

pub(super) fn body<'a>(
    source: &str,
    spans: impl Iterator<Item = &'a Span>,
    from: usize,
    width: usize,
    mode: WidthMode,
) -> Vec<Line> {
    let (width, padding, content_width, spaces) = dimensions(width);
    let mut output = Vec::new();
    let mut line = Line::default();
    line.append(&spaces[..padding], BODY, mode);
    let mut spans = spans.peekable();
    let mut content_column = 0;
    for (relative, grapheme) in source[from..].grapheme_indices(true) {
        let offset = from + relative;
        while spans.peek().is_some_and(|span| span.range.end <= offset) {
            spans.next();
        }
        let style = spans
            .peek()
            .filter(|span| span.range.start <= offset)
            .map(|span| span.style)
            .unwrap_or(BODY);
        if matches!(grapheme, "\n" | "\r\n") {
            fill(&mut line, width, BODY, mode, &spaces);
            output.push(line);
            line = Line::default();
            line.append(&spaces[..padding], BODY, mode);
            content_column = 0;
            continue;
        }
        // Segment the original source, not token spans, so highlighting never
        // separates combining characters or ZWJ emoji at a token boundary.
        let expanded = if grapheme == "\t" { "    " } else { grapheme };
        for part in expanded.graphemes(true) {
            let size = mode.width(part);
            if content_column > 0 && content_column + size > content_width {
                fill(&mut line, width, BODY, mode, &spaces);
                output.push(line);
                line = Line::default();
                line.append(&spaces[..padding], BODY, mode);
                content_column = 0;
            }
            line.append(part, style, mode);
            content_column += size;
        }
    }
    if source[from..].is_empty() || !source.ends_with('\n') {
        fill(&mut line, width, BODY, mode, &spaces);
        output.push(line);
    }
    output
}

fn fill(line: &mut Line, width: usize, style: Style, mode: WidthMode, spaces: &str) {
    let remaining = width.saturating_sub(line.width);
    line.append(&spaces[..remaining], style, mode);
}
