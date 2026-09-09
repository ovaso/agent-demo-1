use std::ops::Range;

use super::panel::BODY;
use crate::ansi::Style;

pub(super) struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

pub(super) struct Highlighted<'a> {
    pub completed: &'a [Span],
    pub preview: Vec<Span>,
}

impl Highlighted<'_> {
    pub(super) fn plain(source: &str) -> Self {
        Self {
            completed: &[],
            preview: vec![Span {
                range: 0..source.len(),
                style: BODY,
            }],
        }
    }

    pub(super) fn spans(&self) -> impl Iterator<Item = &Span> {
        self.completed.iter().chain(&self.preview)
    }
}

#[derive(Default)]
pub(super) struct SyntaxCache {
    #[cfg(feature = "syntax-highlighting")]
    pub(super) session: Option<super::syntax::Session>,
}

impl SyntaxCache {
    pub(super) fn highlight<'a>(
        &'a mut self,
        token: &str,
        source: &str,
        enabled: bool,
    ) -> Highlighted<'a> {
        #[cfg(feature = "syntax-highlighting")]
        {
            if enabled
                && !source.is_empty()
                && !matches!(token, "" | "text" | "plain" | "plaintext" | "txt")
            {
                return super::syntax::highlight(&mut self.session, token, source);
            }
            self.session = None;
        }
        #[cfg(not(feature = "syntax-highlighting"))]
        let _ = (token, enabled);
        Highlighted::plain(source)
    }
}
