use crate::{
    WidthMode,
    ansi::{Line, Style},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(super) struct Cell {
    pub(super) text: String,
    spans: Vec<(usize, Style)>,
}
impl Cell {
    pub(super) fn push(&mut self, text: &str, style: Style) {
        for part in text.split_inclusive('\n') {
            let body = part.strip_suffix('\n').unwrap_or(part);
            if body.chars().any(char::is_control) {
                let mut normalized = Line::default();
                normalized.append(body, style, WidthMode::Unicode);
                self.text.push_str(&normalized.plain);
            } else {
                self.text.push_str(body);
            }
            if part.ends_with('\n') {
                self.text.push('\n');
            }
        }
        if let Some((end, _)) = self.spans.last_mut().filter(|(_, last)| *last == style) {
            *end = self.text.len();
        } else if !text.is_empty() {
            self.spans.push((self.text.len(), style));
        }
    }
    pub(super) fn append(&mut self, other: &Self, bold: bool) {
        let mut start = 0;
        for &(end, mut style) in &other.spans {
            style.bold |= bold;
            self.push(&other.text[start..end], style);
            start = end;
        }
    }
    pub(super) fn wrap(&self, width: usize, mode: WidthMode, bold: bool) -> Vec<Line> {
        let mut lines = vec![Line::default()];
        let mut spans = self.spans.iter();
        let mut current = spans.next();
        for (index, grapheme) in self.text.grapheme_indices(true) {
            if grapheme == "\n" {
                lines.push(Line::default());
                continue;
            }
            while current.is_some_and(|(end, _)| *end <= index) {
                current = spans.next();
            }
            let mut style = current.map_or(Style::PLAIN, |(_, style)| *style);
            style.bold |= bold;
            if lines
                .last()
                .is_some_and(|line| line.width > 0 && line.width + mode.width(grapheme) > width)
            {
                lines.push(Line::default());
            }
            lines
                .last_mut()
                .expect("one line exists")
                .append(grapheme, style, mode);
        }
        lines
    }
}
