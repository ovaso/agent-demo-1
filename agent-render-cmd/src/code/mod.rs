//! Code panels and optional syntax highlighting, implemented for this renderer.
//! Syntect is a direct dependency; none of this module is ported from Go.

use crate::{Options, ansi::Line};
use highlight::SyntaxCache;

mod highlight;
mod panel;
#[cfg(feature = "syntax-highlighting")]
mod syntax;
#[cfg(test)]
mod tests;

const MAX_CACHED_BLOCKS: usize = 8;

#[derive(Default)]
pub(crate) struct CodeBlocks {
    entries: Vec<SyntaxCache>,
    next: usize,
}

impl CodeBlocks {
    pub(crate) fn begin(&mut self) {
        self.next = 0;
    }

    pub(crate) fn render(
        &mut self,
        info: &str,
        source: &str,
        width: usize,
        options: &Options,
    ) -> Vec<Line> {
        let token = info.split_whitespace().next().unwrap_or("text");
        let index = self.next;
        self.next += 1;
        if index < MAX_CACHED_BLOCKS {
            self.entries
                .resize_with(self.entries.len().max(index + 1), SyntaxCache::default);
            let highlighted = self.entries[index].highlight(token, source, options.highlight_code);
            panel::render(
                token,
                source,
                highlighted.spans(),
                width,
                options.width_mode,
            )
        } else {
            let mut cache = SyntaxCache::default();
            let highlighted = cache.highlight(token, source, options.highlight_code);
            panel::render(
                token,
                source,
                highlighted.spans(),
                width,
                options.width_mode,
            )
        }
    }

    pub(crate) fn end(&mut self) {
        self.entries.truncate(self.next.min(MAX_CACHED_BLOCKS));
    }

    #[cfg(all(test, feature = "syntax-highlighting"))]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.next = 0;
    }
}

mod stream;
pub(crate) use stream::StreamingCode;
