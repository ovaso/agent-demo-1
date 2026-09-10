//! Our pulldown-cmark table adapter and column allocation. Glamour's table.go
//! delegates to Go Lip Gloss; its table implementation is not ported here.

use super::link::Link;
use pulldown_cmark::{Alignment, Event, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    WidthMode,
    ansi::{Line, Style},
    upstream::glamour::Theme,
};

mod cell;
use cell::Cell;

#[derive(Clone)]
pub(crate) struct TableFormat {
    widths: Vec<usize>,
    stacked: bool,
}

pub(super) struct Table<'a> {
    alignments: Vec<Alignment>,
    options: crate::Options,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Cell,
    style: Style,
    styles: Vec<Style>,
    links: Vec<Link<'a>>,
}

impl<'a> Table<'a> {
    pub fn new(alignments: Vec<Alignment>, options: &crate::Options) -> Self {
        Self {
            alignments,
            options: options.clone(),
            rows: Vec::new(),
            row: Vec::new(),
            cell: Cell::default(),
            style: Style::PLAIN,
            styles: Vec::new(),
            links: Vec::new(),
        }
    }

    pub fn event(&mut self, event: Event<'a>) {
        match event {
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.text(&text, self.style)
            }
            Event::Code(text) => self.text(&text, Theme::CODE),
            Event::InlineMath(source) => self.math(&source, false),
            Event::DisplayMath(source) => self.math(&source, true),
            Event::SoftBreak | Event::HardBreak => self.text("\n", self.style),
            Event::Start(tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough)) => {
                self.styles.push(self.style);
                match tag {
                    Tag::Strong => self.style.bold = true,
                    Tag::Emphasis => self.style.italic = true,
                    Tag::Strikethrough => self.style.strike = true,
                    _ => unreachable!(),
                }
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                self.style = self.styles.pop().unwrap_or(Style::PLAIN);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.styles.push(self.style);
                self.links.push(Link::new(dest_url, false));
                self.style = Theme::LINK_TEXT;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.styles.push(self.style);
                self.links.push(Link::new(dest_url, true));
                self.style = Theme::LINK_TEXT;
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                self.style = self.styles.pop().unwrap_or(Style::PLAIN);
                if let Some(url) = self.links.pop().and_then(Link::destination) {
                    self.text(" (", self.style);
                    self.text(&url, Theme::LINK);
                    self.text(")", self.style);
                }
            }
            Event::FootnoteReference(label) => self.text(&format!("[^{label}]"), self.style),
            Event::TaskListMarker(checked) => {
                self.text(if checked { "[✓] " } else { "[ ] " }, self.style)
            }
            Event::End(TagEnd::TableCell) => self.row.push(std::mem::take(&mut self.cell)),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                self.rows.push(std::mem::take(&mut self.row))
            }
            _ => {}
        }
    }

    fn math(&mut self, source: &str, display: bool) {
        let text = crate::math::inline(source, &self.options).unwrap_or_else(|| {
            let delimiter = if display { "$$" } else { "$" };
            format!("{delimiter}{source}{delimiter}")
        });
        self.text(&text, self.style);
    }

    fn text(&mut self, text: &str, style: Style) {
        for link in &mut self.links {
            link.observe(text);
        }
        self.cell.push(text, style);
    }

    pub fn render(self, available: usize, mode: WidthMode) -> Vec<Line> {
        self.formatted(available, mode, None, true).1
    }

    pub(super) fn formatted(
        self,
        available: usize,
        mode: WidthMode,
        fixed: Option<&TableFormat>,
        include_header: bool,
    ) -> (TableFormat, Vec<Line>) {
        let count = self.alignments.len();
        if count == 0 {
            return (
                TableFormat {
                    widths: Vec::new(),
                    stacked: false,
                },
                Vec::new(),
            );
        }
        let cells = &self.rows;
        let mut widths = vec![2; count];
        let mut minimum = vec![2; count];
        for row in cells {
            for (column, cell) in row.iter().enumerate().take(count) {
                for line in cell.text.split('\n') {
                    let width = line.graphemes(true).map(|g| mode.width(g)).sum::<usize>();
                    widths[column] = widths[column].max(width);
                    minimum[column] = minimum[column].max(
                        line.graphemes(true)
                            .map(|g| mode.width(g))
                            .max()
                            .unwrap_or(1),
                    );
                }
            }
        }
        let separators = 3 * count.saturating_sub(1);
        if let Some(fixed) = fixed
            && !fixed.stacked
            && minimum
                .iter()
                .zip(&fixed.widths)
                .any(|(needed, width)| needed > width)
        {
            return (
                fixed.clone(),
                stacked(cells, available, mode, include_header),
            );
        }
        if fixed.is_some_and(|f| f.stacked)
            || (fixed.is_none() && minimum.iter().sum::<usize>() + separators > available)
        {
            return (
                TableFormat {
                    widths: Vec::new(),
                    stacked: true,
                },
                stacked(cells, available, mode, include_header),
            );
        }
        if let Some(fixed) = fixed {
            widths.clone_from(&fixed.widths);
        }
        let budget = available - separators;
        let mut total = widths.iter().sum::<usize>();
        while fixed.is_none() && total > budget {
            let column = (0..count)
                .filter(|&i| widths[i] > minimum[i])
                .max_by_key(|&i| widths[i])
                .expect("minimum widths fit");
            widths[column] -= 1;
            total -= 1;
        }
        let mut output = Vec::new();
        for (index, row) in cells.iter().enumerate() {
            if index == 0 && !include_header {
                continue;
            }
            let wrapped: Vec<_> = (0..count)
                .map(|i| {
                    row.get(i).map_or_else(
                        || vec![Line::default()],
                        |cell| cell.wrap(widths[i], mode, index == 0),
                    )
                })
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for y in 0..height {
                let mut line = Line::default();
                for x in 0..count {
                    if x > 0 {
                        line.append(" │ ", Style::PLAIN, mode);
                    }
                    let text = wrapped[x].get(y);
                    let used = text.map_or(0, |line| line.width);
                    let padding = widths[x].saturating_sub(used);
                    let left = match self.alignments[x] {
                        Alignment::Right => padding,
                        Alignment::Center => padding / 2,
                        _ => 0,
                    };
                    let style = Style {
                        bold: index == 0,
                        ..Style::PLAIN
                    };
                    line.append(&" ".repeat(left), style, mode);
                    if let Some(text) = text {
                        line.append_line(text);
                    }
                    line.append(&" ".repeat(padding - left), style, mode);
                }
                output.push(line);
            }
            if index == 0 {
                let mut line = Line::default();
                for (x, width) in widths.iter().enumerate() {
                    if x > 0 {
                        line.append("─┼─", Style::PLAIN, mode);
                    }
                    line.append(&"─".repeat(*width), Style::PLAIN, mode);
                }
                output.push(line);
            }
        }
        (
            TableFormat {
                widths,
                stacked: false,
            },
            output,
        )
    }
}

fn stacked(
    rows: &[Vec<Cell>],
    available: usize,
    mode: WidthMode,
    include_header: bool,
) -> Vec<Line> {
    let mut output = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index == 0 && !include_header {
            continue;
        }
        for (column, cell) in row.iter().enumerate() {
            let mut content = Cell::default();
            if index > 0 {
                if let Some(heading) = rows.first().and_then(|row| row.get(column)) {
                    content.append(heading, true);
                }
                content.push(": ", Style::PLAIN);
            }
            content.append(cell, index == 0);
            output.extend(content.wrap(available, mode, false));
        }
    }
    output
}

pub(crate) fn format_table(
    source: &str,
    width: usize,
    mode: WidthMode,
    fixed: Option<&TableFormat>,
    include_header: bool,
    options: &crate::Options,
) -> Option<(TableFormat, Vec<Line>)> {
    let mut table = None;
    for event in super::supsub::Events::new(super::parser(source)) {
        match event {
            Event::Start(Tag::Table(alignments)) => table = Some(Table::new(alignments, options)),
            Event::End(TagEnd::Table) => {
                return table.map(|table| table.formatted(width, mode, fixed, include_header));
            }
            _ => {
                if let Some(table) = &mut table {
                    table.event(event);
                }
            }
        }
    }
    None
}

pub(crate) fn is_table_row(source: &str) -> bool {
    super::parser(source).any(|event| event == Event::Start(Tag::TableRow))
}
