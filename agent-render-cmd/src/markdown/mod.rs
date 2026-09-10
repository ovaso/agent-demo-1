//! Our block-level pulldown-cmark adapter. No Go parsing code is copied.

use pulldown_cmark::{Options as ParseOptions, Parser};

use crate::{
    Options,
    ansi::{Line, Style},
    code::CodeBlocks,
    upstream::glamour::Theme,
};
use layout::Layout;

mod layout;
mod link;
mod references;
mod render;
use render::render_context;
mod supsub;
#[cfg(feature = "math")]
pub(crate) use supsub::script_text;
mod table;
pub(crate) use references::References;
pub(crate) use table::{TableFormat, format_table, is_table_row};

#[derive(Clone)]
pub(crate) struct PrefixSpec {
    pub first: String,
    pub rest: String,
    pub indent: usize,
}

pub(crate) fn with_prefix(
    lines: Vec<Line>,
    options: &Options,
    prefixes: &[PrefixSpec],
) -> Vec<Line> {
    let mut layout = Layout::new(options, Theme::DOCUMENT_MARGIN);
    for prefix in prefixes {
        layout.push_prefix(prefix.first.clone(), prefix.rest.clone(), prefix.indent);
    }
    layout.block_lines(lines);
    layout.finish()
}

pub(crate) fn scoped(
    source: &str,
    options: &Options,
    prefixes: &[PrefixSpec],
    literal: bool,
) -> Vec<Line> {
    if literal {
        let mut layout = Layout::new(options, Theme::DOCUMENT_MARGIN);
        for prefix in prefixes {
            layout.push_prefix(prefix.first.clone(), prefix.rest.clone(), prefix.indent);
        }
        layout.text(source, Style::PLAIN);
        layout.finish()
    } else {
        render_context(
            source,
            options,
            &mut CodeBlocks::default(),
            prefixes,
            &mut false,
        )
    }
}

pub(crate) fn parser(source: &str) -> Parser<'_> {
    Parser::new_ext(source, parse_options())
}

fn parse_options() -> ParseOptions {
    ParseOptions::ENABLE_TABLES
        | ParseOptions::ENABLE_STRIKETHROUGH
        | ParseOptions::ENABLE_TASKLISTS
        | ParseOptions::ENABLE_FOOTNOTES
        | ParseOptions::ENABLE_MATH
}
