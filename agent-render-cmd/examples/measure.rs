//! Fixed local fixture and fragment boundaries; no model/network/terminal latency.
use std::{
    hint::black_box,
    io::{self, Write},
    time::Instant,
};

use agent_render_cmd::{Options, Renderer};

#[derive(Default)]
struct Counter {
    bytes: usize,
    writes: usize,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let bytes = black_box(bytes);
        self.bytes += bytes.len();
        self.writes += 1;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let source = include_str!("../tests/fixtures/markdown.md");
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
    for color in [false, true] {
        let options = Options {
            color,
            columns: 80,
            rows: 40,
            ..Options::default()
        };
        let start = Instant::now();
        let mut totals = Counter::default();
        for _ in 0..200 {
            let mut renderer = Renderer::new(Counter::default(), black_box(options.clone()));
            for fragment in &fragments {
                renderer.push(black_box(fragment))?;
            }
            renderer.finish()?;
            let counter = black_box(renderer.into_inner());
            totals.bytes += counter.bytes;
            totals.writes += counter.writes;
        }
        println!(
            "color={color}, documents=200, bytes_per_document={}, fragments_per_document={}, elapsed_us={}, output_bytes={}, writes={}",
            source.len(),
            fragments.len(),
            start.elapsed().as_micros(),
            totals.bytes,
            totals.writes
        );
    }
    Ok(())
}
