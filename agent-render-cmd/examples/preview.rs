use agent_render_cmd::{Options, Renderer};
use std::io::{self, Read};

fn main() -> io::Result<()> {
    let mut source = String::new();
    io::stdin().read_to_string(&mut source)?;
    let mut args = std::env::args().skip(1);
    let columns = args.next().and_then(|s| s.parse().ok()).unwrap_or(80);
    let rows = args.next().and_then(|s| s.parse().ok()).unwrap_or(40);
    let characters = args
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let options = Options {
        color: true,
        columns,
        rows,
        ..Options::default()
    };
    let mut renderer = Renderer::new(io::stdout().lock(), options);
    if characters == 0 {
        for line in source.split_inclusive('\n') {
            renderer.push(line)?;
        }
    } else {
        let mut start = 0;
        for (index, (end, _)) in source.char_indices().enumerate() {
            if index > 0 && index % characters == 0 {
                renderer.push(&source[start..end])?;
                start = end;
            }
        }
        renderer.push(&source[start..])?;
    }
    renderer.finish()?;
    println!();
    Ok(())
}
