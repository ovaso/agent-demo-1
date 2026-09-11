//! Application adapter: environment/terminal probes stay outside the render crate.

use std::io::{self, Write};

use agent_core::model::ModelStreamEvent;
use agent_render_cmd::{Options, Renderer, WidthMode};
use rustyline::{Config, GraphemeClusterMode};
use terminal_size::{Height, Width, terminal_size_of};

pub(super) struct StreamingOutput<W> {
    renderer: Option<Renderer<W>>,
    reasoning: Option<bool>,
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
            renderer: Some(Renderer::new(writer, options)),
            reasoning: None,
        }
    }

    pub(super) fn push_event(&mut self, event: ModelStreamEvent<'_>) -> io::Result<()> {
        let (reasoning, delta) = match event {
            ModelStreamEvent::TextDelta(text) => (false, text),
            ModelStreamEvent::ReasoningDelta(text) => (true, text),
        };
        if delta.is_empty() {
            return Ok(());
        }
        if self.reasoning != Some(reasoning) {
            if reasoning || self.reasoning.is_some() {
                let mut renderer = self.renderer.take().expect("active renderer");
                renderer.finish()?;
                let mut writer = renderer.into_inner();
                writeln!(writer, "\n[{}]", if reasoning { "思考" } else { "回答" })?;
                // Each channel gets a fresh Markdown state, including unterminated fences.
                self.renderer = Self::new(writer).renderer;
            }
            self.reasoning = Some(reasoning);
        }
        let (columns, rows) = dimensions();
        let renderer = self.renderer.as_mut().expect("active renderer");
        renderer.resize(columns, rows)?;
        renderer.push(delta)
    }

    pub(super) fn finish(&mut self) -> io::Result<()> {
        if let Some(renderer) = &mut self.renderer {
            renderer.finish()?;
        }
        Ok(())
    }
}

fn dimensions() -> (usize, usize) {
    terminal_size_of(io::stdout())
        .map(|(Width(width), Height(height))| (usize::from(width), usize::from(height)))
        .unwrap_or((0, 0))
}
