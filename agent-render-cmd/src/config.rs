use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::upstream::glow;

/// Match the terminal's handling of grapheme clusters.
#[derive(Clone, Copy, Debug, Default)]
pub enum WidthMode {
    #[default]
    Unicode,
    WcWidth,
    NoZwj,
}

impl WidthMode {
    pub(crate) fn width(self, grapheme: &str) -> usize {
        match self {
            Self::Unicode => grapheme.width(),
            Self::WcWidth => grapheme.chars().map(|c| c.width().unwrap_or(0)).sum(),
            Self::NoZwj => grapheme.split('\u{200d}').map(str::width).sum(),
        }
    }
}

/// Explicit rendering configuration. No environment variables or TTY probes are used.
#[derive(Clone, Debug)]
pub struct Options {
    /// False preserves incoming Markdown byte for byte, including its newlines.
    pub color: bool,
    pub columns: usize,
    pub rows: usize,
    /// None uses Glow's automatic width policy, capped at 120 columns.
    pub wrap_width: Option<usize>,
    pub width_mode: WidthMode,
    /// Enable code highlighting when the syntax-highlighting feature is compiled.
    pub highlight_code: bool,
    /// Render LaTeX when the optional math feature is compiled.
    pub render_math: bool,
    /// Target bound for local speculative text; container/table/highlight metadata is separate.
    pub max_pending_bytes: usize,
    /// Maximum editable preview rows. Committed block output may scroll beyond this.
    pub max_preview_rows: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            color: false,
            columns: 80,
            rows: 24,
            wrap_width: None,
            width_mode: WidthMode::Unicode,
            highlight_code: true,
            render_math: true,
            max_pending_bytes: 32 * 1024,
            max_preview_rows: 32,
        }
    }
}

impl Options {
    pub(crate) fn width(&self) -> usize {
        self.wrap_width
            .unwrap_or_else(|| glow::auto_width(Some(self.columns)))
            .clamp(1, self.columns.max(1))
    }

    pub(crate) fn preview_rows(&self) -> usize {
        self.max_preview_rows.min(self.rows.saturating_sub(3))
    }
}
