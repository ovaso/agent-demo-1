use super::*;
use crate::{WidthMode, ansi::Style};
use highlight::SyntaxCache;

fn options() -> Options {
    Options {
        color: true,
        ..Options::default()
    }
}

#[test]
fn code_panels_are_filled_rectangles_with_padding_and_language_labels() {
    let mut codes = CodeBlocks::default();
    let lines = codes.render("python", "x = 1\n\nprint(x)\n", 36, &options());
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0].plain.trim(), "python");
    assert!(lines.iter().all(|line| line.width == 36));
    assert!(lines[1].plain.starts_with("  x = 1"));
    assert!(lines[2].plain.trim().is_empty());
    assert!(lines[3].plain.starts_with("  print(x)"));
    assert!(lines.last().unwrap().plain.trim().is_empty());
    assert!(
        lines
            .iter()
            .all(|line| !line.plain.contains(['┌', '│', '└']))
    );
    for line in &lines[1..] {
        assert!(line.ansi.contains("\x1b[48;2;35;40;48m"));
    }
}

#[test]
fn narrow_code_panels_wrap_unicode_and_keep_indentation() {
    let mut codes = CodeBlocks::default();
    let source = "中文e\u{301}👩\u{200d}💻世界";
    let lines = codes.render("text", source, 10, &options());
    assert!(lines.iter().all(|line| line.width == 10));
    let rebuilt = lines[1..lines.len() - 1]
        .iter()
        .map(|line| line.plain.trim())
        .collect::<String>();
    assert_eq!(rebuilt, source);
    let lines = codes.render("text", "    indented\n", 30, &options());
    assert!(lines[1].plain.starts_with("      indented"));
}

#[test]
fn unknown_languages_and_disabled_highlighting_keep_literal_code() {
    let mut cache = SyntaxCache::default();
    for (token, enabled) in [
        ("text", true),
        ("unknown_language_xyz", true),
        ("python", false),
    ] {
        let source = "<tag> &amp; **literal**";
        let highlighted = cache.highlight(token, source, enabled);
        assert_eq!(highlighted.spans().count(), 1);
        let span = highlighted.spans().next().unwrap();
        assert_eq!(span.style, panel::BODY);
        assert_eq!(&source[span.range.clone()], source);
    }
}

#[test]
fn empty_panels_and_tiny_widths_still_preserve_text() {
    let mut codes = CodeBlocks::default();
    let empty = codes.render("", "", 20, &options());
    assert_eq!(empty.len(), 3);
    assert_eq!(empty[0].plain.trim(), "text");
    let lines = codes.render("text", "abc", 4, &options());
    assert!(lines.iter().all(|line| line.width == 4));
    assert_eq!(
        lines[1..lines.len() - 1]
            .iter()
            .map(|line| line.plain.trim())
            .collect::<String>(),
        "abc"
    );
}

#[test]
fn token_boundaries_do_not_split_grapheme_clusters() {
    use highlight::Span;
    let source = "e\u{301}";
    let spans = [
        Span {
            range: 0..1,
            style: panel::BODY,
        },
        Span {
            range: 1..source.len(),
            style: Style {
                bold: true,
                ..panel::BODY
            },
        },
    ];
    let lines = panel::render("text", source, spans.iter(), 8, WidthMode::Unicode);
    assert_eq!(lines[1].plain.trim(), source);
    assert_eq!(lines[1].width, 8);
}

#[cfg(feature = "syntax-highlighting")]
mod syntax_tests {
    use super::*;

    fn styles(
        cache: &mut SyntaxCache,
        token: &str,
        source: &str,
    ) -> Vec<(std::ops::Range<usize>, Style)> {
        cache
            .highlight(token, source, true)
            .spans()
            .map(|span| (span.range.clone(), span.style))
            .collect()
    }

    #[test]
    fn common_languages_have_syntax_definitions_and_colored_tokens() {
        for (token, source) in [
            ("python", "def hello():\n    return \"hello\"\n"),
            ("js", "const value = \"hello\"; // comment\n"),
            ("json", "{\"name\": true, \"count\": 1}\n"),
            ("rust", "fn main() { let value = \"hello\"; }\n"),
            ("c", "int main(void) { return 0; }\n"),
            ("bash", "echo \"hello\" # comment\n"),
            ("yaml", "name: \"hello\"\ncount: 1\n"),
            ("diff", "+added\n-removed\n"),
        ] {
            let mut cache = SyntaxCache::default();
            let spans = styles(&mut cache, token, source);
            assert!(cache.session.is_some(), "missing syntax: {token}");
            let mut colors = Vec::new();
            for (_, style) in spans {
                if !colors.contains(&style.fg) {
                    colors.push(style.fg);
                }
            }
            assert!(colors.len() >= 2, "no syntax colors: {token} {colors:?}");
        }
    }

    #[test]
    fn completed_lines_are_not_rehighlighted_on_partial_updates() {
        let mut cache = SyntaxCache::default();
        styles(&mut cache, "python", "x = 1\n");
        assert_eq!(cache.session.as_ref().unwrap().complete_line_calls, 1);
        for tail in ["\"", "\"\"", "\"\"\"open", "value = 2"] {
            let source = format!("x = 1\n{tail}");
            let cached = styles(&mut cache, "python", &source);
            assert_eq!(cache.session.as_ref().unwrap().complete_line_calls, 1);
            assert_eq!(
                cached,
                styles(&mut SyntaxCache::default(), "python", &source)
            );
        }
        styles(&mut cache, "python", "x = 1\nvalue = 2\n");
        assert_eq!(cache.session.as_ref().unwrap().complete_line_calls, 2);
    }

    #[test]
    fn multiline_comment_state_survives_stream_fragments() {
        let mut cache = SyntaxCache::default();
        styles(&mut cache, "rust", "/* open\n");
        let source = "/* open\nstill comment\n*/\nlet value = 1;\n";
        let cached = styles(&mut cache, "rust", source);
        assert_eq!(cached, styles(&mut SyntaxCache::default(), "rust", source));
        let at = |offset| {
            cached
                .iter()
                .find(|(range, _)| range.contains(&offset))
                .unwrap()
                .1
        };
        assert_ne!(
            at(source.find("still").unwrap()).fg,
            at(source.find("let").unwrap()).fg
        );
    }

    #[test]
    fn earlier_source_changes_reset_the_checkpoint() {
        let mut cache = SyntaxCache::default();
        styles(&mut cache, "python", "\"\"\"open\n");
        let source = "value = 1\n";
        assert_eq!(
            styles(&mut cache, "python", source),
            styles(&mut SyntaxCache::default(), "python", source)
        );
    }

    #[test]
    fn multiple_code_blocks_keep_separate_bounded_caches() {
        let mut codes = CodeBlocks::default();
        for tail in ["const", "const value", "const value = 1;"] {
            codes.begin();
            codes.render("python", "x = 1\n", 60, &options());
            codes.render("js", &format!("// comment\n{tail}"), 60, &options());
            codes.end();
            assert_eq!(
                codes.entries[0]
                    .session
                    .as_ref()
                    .unwrap()
                    .complete_line_calls,
                1
            );
            assert_eq!(
                codes.entries[1]
                    .session
                    .as_ref()
                    .unwrap()
                    .complete_line_calls,
                1
            );
        }
        codes.begin();
        for _ in 0..MAX_CACHED_BLOCKS + 3 {
            codes.render("python", "x = 1\n", 60, &options());
        }
        codes.end();
        assert_eq!(codes.entries.len(), MAX_CACHED_BLOCKS);
        codes.clear();
        assert!(codes.entries.is_empty());
    }

    #[test]
    fn oversized_lines_use_plain_text_without_dropping_bytes() {
        let mut cache = SyntaxCache::default();
        let source = format!("{}\n", "x".repeat(9000));
        let spans = styles(&mut cache, "python", &source);
        assert_eq!(spans, vec![(0..source.len(), panel::BODY)]);
    }
}
