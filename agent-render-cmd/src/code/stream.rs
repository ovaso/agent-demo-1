//! A code block's lifetime state; no previous source lines are retained.
use super::{highlight::Span, panel};
use crate::{Options, ansi::Line};

pub(crate) struct StreamingCode {
    label: String,
    disabled: bool,
    #[cfg(feature = "syntax-highlighting")]
    state: Option<super::syntax::StreamingState>,
}

impl StreamingCode {
    pub(crate) fn new(info: &str) -> Self {
        Self {
            label: info.split_whitespace().next().unwrap_or("text").to_owned(),
            disabled: false,
            #[cfg(feature = "syntax-highlighting")]
            state: None,
        }
    }
    pub(crate) fn disable_highlighting(&mut self) {
        self.disabled = true;
    }
    pub(crate) fn header(&self, width: usize, options: &Options) -> Line {
        panel::header(&self.label, width, options.width_mode)
    }
    pub(crate) fn footer(&self, width: usize, options: &Options) -> Line {
        panel::footer(width, options.width_mode)
    }
    pub(crate) fn line_from(
        &mut self,
        source: &str,
        commit: bool,
        from: usize,
        width: usize,
        options: &Options,
    ) -> Vec<Line> {
        #[cfg(feature = "syntax-highlighting")]
        let spans = if options.highlight_code
            && !self.disabled
            && !matches!(self.label.as_str(), "text" | "txt" | "plain" | "plaintext")
        {
            super::syntax::stream_line(&mut self.state, &self.label, source, commit)
        } else {
            vec![Span {
                range: 0..source.len(),
                style: panel::BODY,
            }]
        };
        #[cfg(not(feature = "syntax-highlighting"))]
        let spans = {
            let _ = commit;
            vec![Span {
                range: 0..source.len(),
                style: panel::BODY,
            }]
        };
        panel::body(source, spans.iter(), from, width, options.width_mode)
    }
}
