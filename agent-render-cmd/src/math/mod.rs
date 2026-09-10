//! Optional LaTeX parsing and terminal layout. APIs accept explicit configuration.
#[cfg(feature = "math")]
use crate::ansi::Style;
use crate::{Options, ansi::Line};
#[cfg(feature = "math")]
mod budget;
#[cfg(feature = "math")]
mod compact;
#[cfg(feature = "math")]
mod parse;

pub(crate) fn inline(source: &str, options: &Options) -> Option<String> {
    #[cfg(feature = "math")]
    if options.render_math {
        if source.starts_with(char::is_whitespace) || source.ends_with(char::is_whitespace) {
            return None;
        }
        let node = parse::parse(source, false)?;
        if !budget::fits(&node) {
            return None;
        }
        return compact::render(&node).map(|text| text.trim().to_owned());
    }
    let _ = (source, options);
    None
}
pub(crate) fn display(source: &str, width: usize, options: &Options) -> Option<Vec<Line>> {
    #[cfg(feature = "math")]
    if options.render_math {
        let node = parse::parse(source, true)?;
        if !budget::fits(&node) {
            return None;
        }
        let block = term_maths::layout::layout(&node);
        if block.width() > width || block.height() > 64 {
            return None;
        }
        let mut lines = Vec::new();
        for row in block.cells() {
            let mut line = Line::default();
            for cell in row {
                line.append(cell, Style::PLAIN, options.width_mode);
            }
            if line.width > width {
                return None;
            }
            lines.push(line);
        }
        return Some(lines);
    }
    let _ = (source, width, options);
    None
}
