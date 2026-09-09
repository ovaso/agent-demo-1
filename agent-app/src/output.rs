//! Application adapter: environment/terminal probes stay outside the render crate.

use std::io::{self, Write};

use agent_render_cmd::{Options, Renderer, WidthMode};
use rustyline::{Config, GraphemeClusterMode};
use terminal_size::{Height, Width, terminal_size_of};

pub(super) struct StreamingOutput<W> {
    renderer: Renderer<W>,
}

impl<W: Write> StreamingOutput<W> {
    pub(super) fn new(writer: W) -> Self {
        let (columns, rows) = dimensions();
        let width_mode = match Config::default().grapheme_cluster_mode() {
            GraphemeClusterMode::Unicode => WidthMode::Unicode,
            GraphemeClusterMode::WcWidth => WidthMode::WcWidth,
            GraphemeClusterMode::NoZwj => WidthMode::NoZwj,
        };
        let options = Options {
            color: crate::terminal::styles_enabled(),
            columns,
            rows,
            width_mode,
            ..Options::default()
        };
        Self {
            renderer: Renderer::new(writer, options),
        }
    }

    pub(super) fn push(&mut self, delta: &str) -> io::Result<()> {
        let (columns, rows) = dimensions();
        self.renderer.resize(columns, rows)?;
        self.renderer.push(delta)
    }

    pub(super) fn finish(&mut self) -> io::Result<()> {
        self.renderer.finish()
    }
}

fn dimensions() -> (usize, usize) {
    terminal_size_of(io::stdout())
        .map(|(Width(width), Height(height))| (usize::from(width), usize::from(height)))
        .unwrap_or((0, 0))
}
