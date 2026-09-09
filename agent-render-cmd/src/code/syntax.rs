//! Incremental syntect adapter. Completed lines own the committed parser state;
//! unfinished text is highlighted from a temporary copy of that checkpoint.

use std::sync::LazyLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, HighlightState, Theme, ThemeSet},
    parsing::{ParseState, SyntaxReference, SyntaxSet},
};

use super::{
    highlight::{Highlighted, Span},
    panel::BODY,
};
use crate::ansi::{Color, Style};

const MAX_HIGHLIGHT_LINE_BYTES: usize = 8 * 1024;

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}
static ASSETS: LazyLock<Assets> = LazyLock::new(|| Assets {
    syntaxes: SyntaxSet::load_defaults_newlines(),
    theme: ThemeSet::load_defaults()
        .themes
        .remove("base16-ocean.dark")
        .expect("bundled syntect theme"),
});

pub(super) struct Session {
    token: String,
    committed: String,
    spans: Vec<Span>,
    state: Option<(HighlightState, ParseState)>,
    failed: bool,
    #[cfg(test)]
    pub(super) complete_line_calls: usize,
}

impl Session {
    fn new(token: &str, syntax: &SyntaxReference) -> Self {
        Self {
            token: token.to_owned(),
            committed: String::new(),
            spans: Vec::new(),
            state: Some(HighlightLines::new(syntax, &ASSETS.theme).state()),
            failed: false,
            #[cfg(test)]
            complete_line_calls: 0,
        }
    }
}

fn syntax_for(token: &str) -> Option<&'static SyntaxReference> {
    let alias = match token {
        "bash" | "shell" | "console" => "sh",
        "javascript" => "js",
        "typescript" => "ts",
        "c++" => "cpp",
        "c#" => "cs",
        "yml" => "yaml",
        "py" => "python",
        other => other,
    };
    ASSETS.syntaxes.find_syntax_by_token(alias)
}

pub(super) fn highlight<'a>(
    slot: &'a mut Option<Session>,
    token: &str,
    source: &str,
) -> Highlighted<'a> {
    let Some(syntax) = syntax_for(token) else {
        *slot = None;
        return Highlighted::plain(source);
    };
    if slot
        .as_ref()
        .is_none_or(|s| s.token != token || !source.starts_with(&s.committed))
    {
        *slot = Some(Session::new(token, syntax));
    }
    let session = slot.as_mut().expect("initialized above");
    if session.failed {
        return Highlighted::plain(source);
    }
    let complete_end = source.rfind('\n').map_or(0, |offset| offset + 1);
    let mut offset = session.committed.len();
    let state = session
        .state
        .take()
        .expect("checkpoint is restored after each call");
    let mut highlighter = HighlightLines::from_state(&ASSETS.theme, state.0, state.1);
    for line in source[offset..complete_end].split_inclusive('\n') {
        #[cfg(test)]
        {
            session.complete_line_calls += 1;
        }
        if line.len() > MAX_HIGHLIGHT_LINE_BYTES
            || !append_spans(&mut highlighter, line, offset, &mut session.spans)
        {
            session.failed = true;
            break;
        }
        session.committed.push_str(line);
        offset += line.len();
    }
    session.state = Some(highlighter.state());
    if session.failed {
        session.committed.clear();
        session.committed.push_str(source);
        session.spans.clear();
        return Highlighted::plain(source);
    }
    let mut preview = Vec::new();
    let tail = &source[complete_end..];
    if !tail.is_empty() {
        let (highlight, parse) = session.state.as_ref().expect("checkpoint just restored");
        let mut scratch =
            HighlightLines::from_state(&ASSETS.theme, highlight.clone(), parse.clone());
        if tail.len() > MAX_HIGHLIGHT_LINE_BYTES
            || !append_spans(&mut scratch, tail, complete_end, &mut preview)
        {
            preview.clear();
            preview.push(Span {
                range: complete_end..source.len(),
                style: BODY,
            });
        }
    }
    Highlighted {
        completed: &session.spans,
        preview,
    }
}

fn append_spans(
    highlighter: &mut HighlightLines<'_>,
    line: &str,
    start: usize,
    output: &mut Vec<Span>,
) -> bool {
    let Ok(ranges) = highlighter.highlight_line(line, &ASSETS.syntaxes) else {
        return false;
    };
    let mut offset = start;
    for (style, text) in ranges {
        let end = offset + text.len();
        output.push(Span {
            range: offset..end,
            style: Style {
                fg: Some(Color::Rgb(
                    style.foreground.r,
                    style.foreground.g,
                    style.foreground.b,
                )),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
                underline: style.font_style.contains(FontStyle::UNDERLINE),
                ..BODY
            },
        });
        offset = end;
    }
    true
}

pub(super) struct StreamingState {
    checkpoint: Option<(HighlightState, ParseState)>,
    failed: bool,
}

pub(super) fn stream_line(
    slot: &mut Option<StreamingState>,
    token: &str,
    source: &str,
    commit: bool,
) -> Vec<Span> {
    if slot.is_none() {
        let checkpoint =
            syntax_for(token).map(|syntax| HighlightLines::new(syntax, &ASSETS.theme).state());
        let failed = checkpoint.is_none();
        *slot = Some(StreamingState { checkpoint, failed });
    }
    let state = slot.as_mut().unwrap();
    let plain = || {
        vec![Span {
            range: 0..source.len(),
            style: BODY,
        }]
    };
    if state.failed || source.len() > MAX_HIGHLIGHT_LINE_BYTES {
        if commit {
            state.failed = true;
        }
        return plain();
    }
    let checkpoint = if commit {
        state.checkpoint.take().unwrap()
    } else {
        state.checkpoint.as_ref().unwrap().clone()
    };
    let mut highlighter = HighlightLines::from_state(&ASSETS.theme, checkpoint.0, checkpoint.1);
    let mut spans = Vec::new();
    let ok = append_spans(&mut highlighter, source, 0, &mut spans);
    if commit {
        state.checkpoint = Some(highlighter.state());
        state.failed = !ok;
    }
    if ok { spans } else { plain() }
}
