//! Late references are a bounded display dependency, independent of the block
//! stack. Only unresolved fragments keep source; intervening code keeps ANSI rows.
use super::output::Output;
use crate::{
    ansi::Line,
    markdown::{self, PrefixSpec},
};
use std::io::{self, Write};

pub(super) struct Piece {
    pub(super) lines: Vec<Line>,
    source: Option<(String, Vec<PrefixSpec>)>,
}
#[derive(Default)]
pub(super) struct Deferred {
    pub(super) pieces: Vec<Piece>,
    pub(super) frozen_unresolved: bool,
}
impl Deferred {
    pub(super) fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
    pub(super) fn unresolved(&self) -> bool {
        self.pieces.iter().any(|p| p.source.is_some())
    }
    pub(super) fn bytes(&self) -> usize {
        self.pieces
            .iter()
            .map(|p| {
                p.lines
                    .iter()
                    .map(|l| l.plain.len() + l.ansi.len())
                    .sum::<usize>()
                    + p.source.as_ref().map_or(0, |(s, prefixes)| {
                        s.len()
                            + prefixes
                                .iter()
                                .map(|p| p.first.len() + p.rest.len())
                                .sum::<usize>()
                    })
            })
            .sum()
    }
    pub(super) fn lines(&self) -> Vec<Line> {
        self.pieces
            .iter()
            .flat_map(|p| p.lines.iter().cloned())
            .collect()
    }
    pub(super) fn ready(&mut self, lines: &[Line]) {
        if lines.is_empty() {
            return;
        }
        if let Some(piece) = self.pieces.last_mut().filter(|p| p.source.is_none()) {
            piece.lines.extend_from_slice(lines);
        } else {
            self.pieces.push(Piece {
                lines: lines.to_vec(),
                source: None,
            });
        }
    }
    pub(super) fn ends_blank(&self) -> bool {
        self.pieces
            .last()
            .and_then(|p| p.lines.last())
            .is_none_or(|l| l.plain.trim().is_empty())
    }
}
impl<W: Write> Output<W> {
    pub(super) fn render_markdown(
        &self,
        source: &str,
        prefixes: &[PrefixSpec],
        literal: bool,
    ) -> Vec<Line> {
        if literal {
            markdown::scoped(source, &self.options, prefixes, true)
        } else {
            self.references.render(source, &self.options, prefixes).0
        }
    }
    pub(super) fn append_markdown(
        &mut self,
        source: &str,
        prefixes: &[PrefixSpec],
        literal: bool,
    ) -> io::Result<()> {
        if literal {
            let lines = self.render_markdown(source, prefixes, true);
            return self.append(&lines);
        }
        let definitions = self
            .references
            .collect(source, self.options.max_pending_bytes);
        if !definitions.is_empty() {
            for piece in &mut self.deferred.pieces {
                if let Some((source, prefixes)) = &piece.source {
                    let (lines, unresolved) =
                        self.references.render(source, &self.options, prefixes);
                    piece.lines = lines;
                    if !unresolved {
                        piece.source = None;
                    }
                }
            }
            if !self.deferred.unresolved() {
                self.flush_pending()?;
            }
        }
        let (lines, unresolved) = self.references.render(source, &self.options, prefixes);
        if unresolved {
            self.deferred.pieces.push(Piece {
                lines,
                source: Some((source.to_owned(), prefixes.to_vec())),
            });
            self.paint_pending()?;
        } else {
            self.append(&lines)?;
        }
        // Once an unresolved fragment has left the editable region, preserve
        // later definitions visibly so its destination never silently vanishes.
        if self.deferred.frozen_unresolved || self.references.overflowed {
            for definition in definitions {
                let lines = markdown::scoped(&definition, &self.options, prefixes, true);
                self.append(&lines)?;
            }
        }
        Ok(())
    }
}
