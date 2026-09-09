use super::{
    leaf::Leaf,
    lex::{self, ListKind, Marker},
};
use crate::{
    Options,
    ansi::{Line, Style},
    markdown::PrefixSpec,
    upstream::glamour::Theme,
};

pub(super) enum Frame {
    Quote,
    List {
        kind: ListKind,
        next: u64,
    },
    Item {
        source_indent: usize,
        marker: String,
        emitted: bool,
    },
    Leaf(Box<Leaf>),
}

#[derive(Default)]
pub(super) struct Stack {
    pub(super) frames: Vec<Frame>,
}

impl Stack {
    pub(super) fn leaf(&self) -> Option<&Leaf> {
        match self.frames.last() {
            Some(Frame::Leaf(leaf)) => Some(leaf),
            _ => None,
        }
    }
    pub(super) fn take_leaf(&mut self) -> Option<Box<Leaf>> {
        if matches!(self.frames.last(), Some(Frame::Leaf(_))) {
            match self.frames.pop() {
                Some(Frame::Leaf(leaf)) => Some(leaf),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }
    pub(super) fn put(&mut self, leaf: impl Into<Box<Leaf>>) {
        self.frames.push(Frame::Leaf(leaf.into()));
    }
    pub(super) fn mark_emitted(&mut self) {
        for frame in &mut self.frames {
            if let Frame::Item { emitted, .. } = frame {
                *emitted = true;
            }
        }
    }
    pub(super) fn prefixes(&self) -> Vec<PrefixSpec> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                Frame::Quote => Some(PrefixSpec {
                    first: Theme::QUOTE_PREFIX.into(),
                    rest: Theme::QUOTE_PREFIX.into(),
                    indent: 2,
                }),
                Frame::Item {
                    marker, emitted, ..
                } => {
                    let indent = marker.chars().count().max(Theme::LIST_INDENT);
                    Some(PrefixSpec {
                        first: if *emitted {
                            " ".repeat(indent)
                        } else {
                            marker.clone()
                        },
                        rest: " ".repeat(indent),
                        indent,
                    })
                }
                _ => None,
            })
            .collect()
    }
    pub(super) fn width(&self, options: &Options) -> usize {
        let margin = Theme::DOCUMENT_MARGIN.min(options.width().saturating_sub(2) / 2);
        options
            .width()
            .saturating_sub(
                2 * margin
                    + self
                        .frames
                        .iter()
                        .map(|f| match f {
                            Frame::Quote => 2,
                            Frame::Item { marker, .. } => {
                                marker.chars().count().max(Theme::LIST_INDENT)
                            }
                            _ => 0,
                        })
                        .sum::<usize>(),
            )
            .max(1)
    }
    pub(super) fn prefix_line(&self, line: Line, options: &Options) -> Line {
        let mut prefix = self.blank(options);
        prefix.append_line(&line);
        prefix
    }
    pub(super) fn blank(&self, options: &Options) -> Line {
        let mut line = Line::default();
        let margin = Theme::DOCUMENT_MARGIN.min(options.width().saturating_sub(2) / 2);
        line.append(&" ".repeat(margin), Style::PLAIN, options.width_mode);
        for prefix in self.prefixes() {
            line.append(&prefix.first, Style::PLAIN, options.width_mode);
        }
        line
    }
    /// Validate continuation against existing containers before looking for new syntax.
    pub(super) fn continuation<'a>(&self, text: &'a str, lazy: bool) -> (usize, &'a str) {
        let mut rest = text;
        for (index, frame) in self.frames.iter().enumerate() {
            match frame {
                Frame::Quote => {
                    if let Some(n) = lex::quote(rest) {
                        rest = &rest[n..];
                    } else if !lazy || rest.trim().is_empty() || lex::interrupts(rest) {
                        return (index, rest);
                    }
                }
                Frame::Item { source_indent, .. } => {
                    let spaces = lex::leading_spaces(rest);
                    if rest.trim().is_empty() { rest = ""; }
                    else if spaces >= *source_indent { rest = &rest[*source_indent..]; }
                    else if !lazy || lex::interrupts(rest) || lex::marker(rest).is_some_and(|marker| matches!(self.frames.get(index.wrapping_sub(1)), Some(Frame::List { kind, .. }) if *kind == marker.kind)) { return (index, rest); }
                }
                _ => {}
            }
        }
        (self.frames.len(), rest)
    }
    pub(super) fn truncate(&mut self, matched: usize) {
        self.frames.truncate(matched);
    }
    pub(super) fn new_item(&mut self, marker: Marker) {
        let number = match self.frames.last_mut() {
            Some(Frame::List { kind, next }) if *kind == marker.kind => {
                let n = *next;
                *next = next.saturating_add(1);
                n
            }
            _ => {
                if matches!(self.frames.last(), Some(Frame::List { .. })) {
                    self.frames.pop();
                }
                let n = marker.number;
                self.frames.push(Frame::List {
                    kind: marker.kind,
                    next: n.saturating_add(1),
                });
                n
            }
        };
        let display = match marker.kind {
            ListKind::Bullet(_) => Theme::ITEM_PREFIX.into(),
            ListKind::Ordered(_) => format!("{number}. "),
        };
        self.frames.push(Frame::Item {
            source_indent: marker.indent,
            marker: display,
            emitted: false,
        });
    }
    pub(super) fn close_unattached_list(&mut self) {
        while matches!(self.frames.last(), Some(Frame::List { .. })) {
            self.frames.pop();
        }
    }
    pub(super) fn is_root(&self) -> bool {
        self.frames.iter().all(|f| matches!(f, Frame::Leaf(_)))
    }
}
