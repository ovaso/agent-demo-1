//! Bounded document definitions. pulldown-cmark retains responsibility for label
//! normalization, duplicate definitions, escapes and reference syntax.
use std::borrow::Cow;

use super::{PrefixSpec, parser, render_context};
use crate::{Options, ansi::Line, code::CodeBlocks};

#[derive(Default)]
pub(crate) struct References {
    source: String,
    pub(crate) overflowed: bool,
}

impl References {
    pub(crate) fn collect(&mut self, source: &str, limit: usize) -> Vec<String> {
        if !source.contains("]:") {
            return Vec::new();
        }
        let parsed = parser(source);
        let mut definitions: Vec<_> = parsed.reference_definitions().iter().collect();
        definitions.sort_unstable_by_key(|(_, def)| def.span.start);
        let mut added = Vec::new();
        for (label, definition) in definitions {
            if parser(&self.source)
                .reference_definitions()
                .get(label)
                .is_some()
            {
                continue;
            }
            let raw = source[definition.span.clone()].trim_end();
            if raw.len() < limit.saturating_sub(self.source.len()) {
                self.source.push_str(raw);
                self.source.push('\n');
            } else {
                self.overflowed = true;
            }
            // Also return definitions that did not fit, so callers can preserve
            // their visible source rather than silently discarding destinations.
            added.push(raw.to_owned());
        }
        added
    }

    pub(crate) fn source<'a>(&self, source: &'a str) -> Cow<'a, str> {
        if self.source.is_empty() || !source.contains('[') {
            Cow::Borrowed(source)
        } else {
            Cow::Owned(format!("{}\n{source}", self.source))
        }
    }

    pub(crate) fn render(
        &self,
        source: &str,
        options: &Options,
        prefixes: &[PrefixSpec],
    ) -> (Vec<Line>, bool) {
        let source = self.source(source);
        let mut unresolved = false;
        let lines = render_context(
            &source,
            options,
            &mut CodeBlocks::default(),
            prefixes,
            &mut unresolved,
        );
        (lines, unresolved)
    }
}
