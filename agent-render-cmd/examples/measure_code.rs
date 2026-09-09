//! Same fixture and fragment boundaries for plain panels, cold and warm highlighting.
use agent_render_cmd::{Options, Renderer};
use std::{
    hint::black_box,
    io::{self, Write},
    time::Instant,
};

#[derive(Default)]
struct Counter {
    bytes: usize,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes += black_box(bytes).len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let source = include_str!("../tests/fixtures/code-panels.md");
    let mut fragments = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let mut end = (start + 32).min(source.len());
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        fragments.push(&source[start..end]);
        start = end;
    }
    for (name, highlight_code, runs) in [
        ("panels", false, 200),
        ("highlight-cold", true, 1),
        ("highlight-warm", true, 200),
    ] {
        let options = Options {
            color: true,
            columns: 80,
            rows: 40,
            highlight_code,
            ..Options::default()
        };
        let start = Instant::now();
        let mut bytes = 0;
        for _ in 0..runs {
            let mut renderer = Renderer::new(Counter::default(), black_box(options.clone()));
            for fragment in &fragments {
                renderer.push(black_box(fragment))?;
            }
            renderer.finish()?;
            bytes += black_box(renderer.into_inner()).bytes;
        }
        println!(
            "{name}: runs={runs}, source_bytes={}, fragments={}, elapsed_us={}, output_bytes={bytes}",
            source.len(),
            fragments.len(),
            start.elapsed().as_micros()
        );
    }
    Ok(())
}
