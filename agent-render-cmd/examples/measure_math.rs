//! Fixed local math fixture, 32-byte fragments and a counter instead of terminal IO.
use agent_render_cmd::{Options, Renderer};
use std::{
    hint::black_box,
    io::{self, Write},
    time::Instant,
};
#[derive(Default)]
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += black_box(bytes).len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn main() -> io::Result<()> {
    let source = include_str!("../tests/fixtures/math.md");
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
    for render_math in [false, true] {
        let begin = Instant::now();
        let mut bytes = 0;
        for _ in 0..100 {
            let mut renderer = Renderer::new(
                Counter::default(),
                Options {
                    color: true,
                    columns: 100,
                    rows: 40,
                    render_math,
                    ..Options::default()
                },
            );
            for delta in &fragments {
                renderer.push(black_box(delta))?;
            }
            renderer.finish()?;
            bytes += renderer.into_inner().0;
        }
        println!(
            "math={render_math}, runs=100, source_bytes={}, fragments={}, elapsed_us={}, output_bytes={bytes}",
            source.len(),
            fragments.len(),
            begin.elapsed().as_micros()
        );
    }
    Ok(())
}
