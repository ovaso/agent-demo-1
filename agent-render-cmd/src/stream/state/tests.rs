use super::*;
use leaf::{CodeEnd, Leaf};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Terminal(Rc<RefCell<vt100::Parser>>);
impl Terminal {
    fn new(rows: u16, columns: u16) -> Self {
        Self(Rc::new(RefCell::new(vt100::Parser::new(rows, columns, 0))))
    }
    fn text(&self) -> String {
        self.0.borrow().screen().contents()
    }
    fn resize(&self, rows: u16, columns: u16) {
        self.0.borrow_mut().screen_mut().set_size(rows, columns);
    }
    fn bg(&self, needle: &str, column: u16) -> vt100::Color {
        let parser = self.0.borrow();
        let screen = parser.screen();
        let row = screen
            .rows(0, screen.size().1)
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("missing {needle}: {}", screen.contents()));
        screen.cell(row as u16, column).unwrap().bgcolor()
    }
}
impl Write for Terminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().process(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn renderer(rows: usize, columns: usize) -> (Renderer<Terminal>, Terminal) {
    let terminal = Terminal::new(rows as u16, columns as u16);
    (
        Renderer::new(
            terminal.clone(),
            Options {
                color: true,
                rows,
                columns,
                ..Options::default()
            },
        ),
        terminal,
    )
}
fn characters(renderer: &mut Renderer<Terminal>, source: &str) {
    for ch in source.chars() {
        renderer.push(ch.encode_utf8(&mut [0; 4])).unwrap();
    }
}

#[test]
fn every_block_family_has_a_persistent_stack_state() {
    let (mut r, _) = renderer(50, 80);
    r.push("# Heading\n").unwrap();
    assert!(r.stack.leaf().is_none());
    r.push("paragraph\n").unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Paragraph(_))));
    r.push("\n| H | V |\n|---|---|\n").unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Table(_))));
    r.push("| x | y |\n\n```rust\n").unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Code(_))));
    r.push("> # literal\n```\n<div>\n").unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Html(_))));
    r.push("raw html\n\n    indented\n").unwrap();
    assert!(
        matches!(r.stack.leaf(), Some(Leaf::Code(code)) if matches!(code.end, CodeEnd::Indent))
    );
    r.push("outside\n").unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Paragraph(_))));
}

#[test]
fn long_code_survives_scrollback_without_retaining_its_source() {
    let (mut r, terminal) = renderer(8, 80);
    r.options.max_pending_bytes = 64;
    r.push("```rust\n").unwrap();
    for n in 0..2000 {
        r.push(&format!("let line{n:04} = 1;\n")).unwrap();
    }
    assert!(matches!(r.stack.leaf(), Some(Leaf::Code(code)) if code.lines == 2000));
    assert_eq!(r.stack.frames.len(), 1);
    assert!(r.raw.is_empty());
    assert!(terminal.text().contains("line1999"));
    assert_eq!(terminal.bg("line1999", 2), vt100::Color::Rgb(35, 40, 48));
    r.push("```\n\nafter code\n").unwrap();
    r.finish().unwrap();
    assert!(terminal.text().contains("after code"));
    assert!(!terminal.text().contains("```"));
    assert_eq!(terminal.bg("after code", 2), vt100::Color::Default);
}

#[test]
fn code_resize_preserves_type_highlighter_and_unfinished_text() {
    let (mut r, terminal) = renderer(20, 80);
    r.push("```python\nvalue = \"hel").unwrap();
    terminal.resize(20, 60);
    r.resize(60, 20).unwrap();
    r.push("lo\"\nreturn value\n```\n").unwrap();
    r.finish().unwrap();
    let text = terminal.text();
    assert_eq!(text.matches("value =").count(), 1, "{text}");
    assert_eq!(text.matches("hel").count(), 1, "{text}");
    assert_eq!(text.matches("lo\"").count(), 1, "{text}");
    assert_eq!(
        terminal.bg("return value", 2),
        vt100::Color::Rgb(35, 40, 48)
    );
    assert!(!text.contains("```"));
}

#[test]
fn code_containers_and_matching_fences_are_not_reparsed_as_body_markup() {
    let (mut r, terminal) = renderer(40, 80);
    characters(
        &mut r,
        "1. outer\n   > ````text\n   > # literal heading\n   > ```\n   > - literal list\n   > ````\n2. next\n",
    );
    r.finish().unwrap();
    let text = terminal.text();
    assert!(
        text.contains("# literal heading") && text.contains("- literal list"),
        "{text}"
    );
    assert!(text.contains("2. next"), "{text}");
    assert_eq!(
        terminal.bg("literal heading", 7),
        vt100::Color::Rgb(35, 40, 48)
    );
}

#[test]
fn omitted_quote_prefix_closes_the_code_and_container() {
    let (mut r, terminal) = renderer(30, 80);
    r.push("> ```text\n> code\noutside\n").unwrap();
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("  outside"), "{text}");
    assert_eq!(terminal.bg("outside", 2), vt100::Color::Default);
}

#[test]
fn long_lists_keep_numbering_and_bounded_container_depth() {
    let (mut r, terminal) = renderer(8, 80);
    for n in 0..1000 {
        r.push(&format!("1. item{n:04}\n")).unwrap();
    }
    assert_eq!(r.stack.frames.len(), 3);
    assert!(matches!(r.stack.leaf(), Some(Leaf::Paragraph(p)) if p.source.len()<32));
    r.finish().unwrap();
    assert!(
        terminal.text().contains("1000. item0999"),
        "{}",
        terminal.text()
    );
}

#[test]
fn nested_lists_quotes_tasks_and_escape_rules_survive_character_fragments() {
    let (mut r, terminal) = renderer(50, 100);
    let source = "1. one\n2. two\n   - nested\n     1. inner\n3. three\n\n> first\n> > second\n\n- [x] done\n- [ ] todo\n\n\\*literal\\* and **bold**\n";
    characters(&mut r, source);
    r.finish().unwrap();
    let text = terminal.text();
    for expected in [
        "  1. one",
        "  2. two",
        "     • nested",
        "       1. inner",
        "  3. three",
        "│ │ second",
        "[✓] done",
        "[ ] todo",
        "*literal* and bold",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}

#[test]
fn table_header_is_not_frozen_by_incomplete_rows() {
    let source = include_str!("../../../tests/fixtures/streamed-table.md");
    let (mut r, terminal) = renderer(30, 100);
    characters(&mut r, source);
    r.finish().unwrap();
    let text = terminal.text();
    for name in ["登录功能", "注册功能", "消息通知"] {
        let row = text.lines().find(|line| line.contains(name)).unwrap();
        assert!(row.contains('│') && !row.contains('|'), "{row}");
    }
}

#[test]
fn long_tables_stream_with_a_retained_schema() {
    let (mut r, terminal) = renderer(10, 80);
    r.push("| ID | Value |\n|---|---|\n").unwrap();
    for n in 0..200 {
        r.push(&format!("| {n} | payload{n:04} |\n")).unwrap();
    }
    assert!(
        matches!(r.stack.leaf(), Some(Leaf::Table(table)) if table.format.is_some() && table.rows.len()<=8)
    );
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("payload0199"), "{text}");
    assert!(!text.contains("| 199 |"), "{text}");
}

#[test]
fn long_paragraphs_and_lines_do_not_drop_text_when_preview_bounds_are_hit() {
    let (mut r, terminal) = renderer(80, 40);
    r.options.max_pending_bytes = 64;
    r.options.max_preview_rows = 2;
    let source = "word".repeat(90);
    characters(&mut r, &source);
    r.finish().unwrap();
    let visible = terminal
        .text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    assert_eq!(visible, source);
}

#[test]
fn html_is_a_distinct_literal_state_and_closes_at_its_terminator() {
    let (mut r, terminal) = renderer(30, 80);
    characters(&mut r, "<!--\n# still a comment\n-->\n# actual heading\n");
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("# still a comment"), "{text}");
    assert!(
        text.contains("actual heading") && !text.contains("# actual heading"),
        "{text}"
    );
}

#[test]
fn multiline_inline_markup_and_reference_definitions_stay_local_to_paragraph() {
    let (mut r, terminal) = renderer(30, 100);
    characters(
        &mut r,
        "**first\nsecond** and [docs][r]\n\n[r]: https://example.com\n\n# next\n",
    );
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("first second"), "{text}");
    assert!(text.contains("docs (https://example.com)"), "{text}");
}

#[test]
fn nearby_reference_definitions_resolve_preceding_list_items() {
    let (mut r, terminal) = renderer(24, 100);
    characters(&mut r, "- [Baidu][ref1]\n- [Google][]\n\n");
    assert!(
        terminal.text().contains("Baidu"),
        "references must be visible while streaming"
    );
    characters(
        &mut r,
        "[ref1]: https://www.baidu.com\n[Google]: https://www.google.com\n",
    );
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("• Baidu (https://www.baidu.com)"), "{text}");
    assert!(text.contains("• Google (https://www.google.com)"), "{text}");
    assert!(
        !text.contains("[ref1]") && !text.contains("[Google][]"),
        "{text}"
    );
}

#[test]
fn footnotes_keep_labels_and_definitions_instead_of_becoming_urls() {
    let (mut r, terminal) = renderer(24, 80);
    characters(&mut r, "句子[^1]。\n\n[^1]: 这是脚注内容。\n");
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("句子[^1]。"), "{text}");
    assert!(text.contains("[^1]: 这是脚注内容。"), "{text}");
    assert!(!text.contains("(这是脚注内容。)"), "{text}");
}

#[test]
fn definitions_survive_blocks_and_use_commonmark_label_matching() {
    let (mut r, terminal) = renderer(30, 110);
    characters(
        &mut r,
        "[Straße]: https://first.example\n[Mixed  Case]: https://mixed.example\n\n# boundary\n\n- [one][STRASSE]\n- ![two][mixed case]\n\n[Straße]: https://second.example\n\n[three][straße]\n",
    );
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("one (https://first.example)"), "{text}");
    assert!(text.contains("two (https://mixed.example)"), "{text}");
    assert!(text.contains("three (https://first.example)"), "{text}");
    assert!(
        !text.contains("second.example"),
        "first definition must win: {text}"
    );
}

#[test]
fn unresolved_references_do_not_buffer_long_code_or_lose_late_destinations() {
    let (mut r, terminal) = renderer(10, 100);
    r.options.max_pending_bytes = 1024;
    r.output.options = r.options.clone();
    characters(&mut r, "- [earlier][r]\n\n```rust\n");
    for n in 0..200 {
        r.push(&format!("let row{n:04} = 1;\n")).unwrap();
        assert!(r.output.deferred.bytes() <= r.options.max_pending_bytes);
    }
    assert!(r.output.deferred.is_empty());
    assert_eq!(terminal.bg("row0199", 2), vt100::Color::Rgb(35, 40, 48));
    characters(&mut r, "```\n\n[r]: https://late.example\n");
    r.finish().unwrap();
    assert!(
        terminal.text().contains("[r]: https://late.example"),
        "{}",
        terminal.text()
    );
}

#[test]
fn reference_definition_budget_preserves_overflow_as_visible_text() {
    let (mut r, terminal) = renderer(20, 100);
    r.options.max_pending_bytes = 64;
    r.output.options = r.options.clone();
    for n in 0..30 {
        r.push(&format!(
            "[ref{n:02}]: https://example.com/{n:02}\n\n# boundary\n\n"
        ))
        .unwrap();
        assert!(r.output.references.bytes() <= 64);
    }
    r.finish().unwrap();
    assert!(
        terminal.text().contains("[ref29]: https://example.com/29"),
        "{}",
        terminal.text()
    );
}

#[test]
fn resize_seals_pending_references_without_duplicating_current_text() {
    let (mut r, terminal) = renderer(24, 100);
    characters(&mut r, "- [early][ref]\n- in progress");
    terminal.resize(24, 80);
    r.resize(80, 24).unwrap();
    characters(&mut r, " continued\n\n[ref]: https://resize.example\n");
    r.finish().unwrap();
    let text = terminal.text();
    assert_eq!(text.matches("early").count(), 1, "{text}");
    assert_eq!(text.matches("in progress").count(), 1, "{text}");
    assert_eq!(text.matches("continued").count(), 1, "{text}");
    assert!(text.contains("https://resize.example"), "{text}");
}

#[test]
fn table_cells_preserve_inline_styles_like_paragraphs() {
    let (mut r, terminal) = renderer(24, 100);
    characters(
        &mut r,
        "| heading | value |\n|---|---|\n| **bold** | `code` |\n\n",
    );
    r.finish().unwrap();
    let parser = terminal.0.borrow();
    let screen = parser.screen();
    let row = screen
        .rows(0, 100)
        .position(|line| line.contains("bold"))
        .unwrap() as u16;
    let cell = (0..100)
        .find_map(|col| screen.cell(row, col).filter(|cell| cell.contents() == "b"))
        .unwrap();
    assert!(cell.bold(), "bold style lost inside table");
    let cell = (0..100)
        .find_map(|col| screen.cell(row, col).filter(|cell| cell.contents() == "c"))
        .unwrap();
    assert_ne!(
        cell.bgcolor(),
        vt100::Color::Default,
        "inline code style lost inside table"
    );
}

#[test]
fn references_footnotes_and_styled_tables_are_independent_of_fragment_boundaries() {
    let source = "- [one][r]\n- [two][]\n\n[r]: https://one.example\n[two]: https://two.example\n\n# table\n\n| heading | value |\n|---|---|\n| **bold** | `code` |\n| *italic* | [one][r] |\n\nFootnote[^1].\n\n[^1]: A **definition**.\n\n```rust\nlet x = 1;\n```\n";
    let (mut r, expected) = renderer(60, 80);
    r.push(source).unwrap();
    r.finish().unwrap();
    let expected = expected.0.borrow().screen().contents_formatted();
    for chunk in [1, 2, 7, 31] {
        let (mut r, terminal) = renderer(60, 80);
        for fragment in source.as_bytes().chunks(chunk) {
            r.push(std::str::from_utf8(fragment).unwrap()).unwrap();
        }
        r.finish().unwrap();
        assert_eq!(
            terminal.0.borrow().screen().contents_formatted(),
            expected,
            "chunk={chunk}"
        );
    }
}

#[test]
fn ordinary_partial_text_is_visible_before_newline() {
    let (mut r, terminal) = renderer(30, 80);
    r.push("hello").unwrap();
    assert!(terminal.text().contains("hello"));
    r.push(" world").unwrap();
    assert!(terminal.text().contains("hello world"));
}

#[test]
fn root_block_spacing_does_not_depend_on_chunk_boundaries() {
    for source in ["# heading\ntext\n", "<div>\nhtml\n</div>\n\ntext\n"] {
        let (mut r, expected) = renderer(20, 80);
        r.push(source).unwrap();
        r.finish().unwrap();
        let (mut r, actual) = renderer(20, 80);
        characters(&mut r, source);
        r.finish().unwrap();
        assert_eq!(actual.text(), expected.text());
    }
}

#[test]
fn sub_sup_tags_render_in_paragraphs_and_tables_across_stream_fragments() {
    let source = "H<sub>2</sub>O / x<sup>2</sup> / 下标<sub>中文</sub> / 上标<sup>中文</sup>\n\n| Formula | Value |\n|---|---|\n| H<sub>2</sub>O | x<sup>n+1</sup> |\n";
    let (mut r, terminal) = renderer(30, 100);
    characters(&mut r, source);
    r.finish().unwrap();
    let text = terminal.text();
    assert!(
        text.contains("H₂O / x² / 下标_{中文} / 上标^{中文}"),
        "{text}"
    );
    assert!(text.contains("xⁿ⁺¹"), "{text}");
    assert!(!text.contains("<sub>") && !text.contains("<sup>"), "{text}");
}

#[test]
fn sub_sup_rendering_and_style_do_not_depend_on_chunk_boundaries() {
    let source = "# x<sup>2</sup> / K<sub>ij</sub><sup>T</sup>\n\n- **H<sub>2</sub>O**\n- 中文<sup>上标</sup> / 1<sup>st</sup>\n\n> n<sub>i+1</sub>\n\n| Key | Value |\n|---|---|\n| *x<sup>2</sup>* | y<sub>3</sub> |\n";
    let (mut r, expected) = renderer(40, 60);
    r.push(source).unwrap();
    r.finish().unwrap();
    let expected = expected.0.borrow().screen().contents_formatted();
    let mut boundaries: Vec<_> = source.char_indices().map(|(i, _)| i).collect();
    boundaries.push(source.len());
    for chunk in [1, 2, 7, 31] {
        let (mut r, terminal) = renderer(40, 60);
        for start in (0..boundaries.len() - 1).step_by(chunk) {
            r.push(
                &source[boundaries[start]..boundaries[(start + chunk).min(boundaries.len() - 1)]],
            )
            .unwrap();
        }
        r.finish().unwrap();
        assert_eq!(
            terminal.0.borrow().screen().contents_formatted(),
            expected,
            "chunk={chunk}"
        );
    }
    let mut r = Renderer::new(Vec::new(), Options::default());
    r.push(source).unwrap();
    r.finish().unwrap();
    assert_eq!(
        r.into_inner(),
        source.as_bytes(),
        "plain output must preserve HTML source"
    );
}

#[test]
fn plain_output_remains_byte_exact_and_finish_is_idempotent() {
    let mut r = Renderer::new(Vec::new(), Options::default());
    let source = "# title\r\n```rust\nlet x=1;\n```";
    for ch in source.chars() {
        r.push(ch.encode_utf8(&mut [0; 4])).unwrap();
    }
    r.finish().unwrap();
    r.finish().unwrap();
    assert_eq!(
        r.push("late").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(r.into_inner(), source.as_bytes());
}

#[test]
fn output_errors_propagate() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut r = Renderer::new(
        Broken,
        Options {
            color: true,
            ..Options::default()
        },
    );
    assert_eq!(
        r.push("hello").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn resizing_a_partial_paragraph_does_not_duplicate_the_prefix() {
    let (mut r, terminal) = renderer(20, 80);
    r.push("hello").unwrap();
    terminal.resize(20, 60);
    r.resize(60, 20).unwrap();
    r.push(" world\n").unwrap();
    r.finish().unwrap();
    let text = terminal.text();
    assert_eq!(text.matches("hello").count(), 1, "{text}");
    assert_eq!(text.matches("world").count(), 1, "{text}");
}

#[test]
fn a_streaming_table_recalculates_its_schema_after_resize() {
    let (mut r, terminal) = renderer(20, 80);
    r.push("| Left | Right |\n|---|---|\n").unwrap();
    for n in 0..10 {
        r.push(&format!("| a{n} | b{n} |\n")).unwrap();
    }
    terminal.resize(20, 12);
    r.resize(12, 20).unwrap();
    assert!(
        matches!(r.stack.leaf(), Some(Leaf::Table(table)) if table.format.is_none() && !table.header_written)
    );
    r.push("| end | value |\n").unwrap();
    r.finish().unwrap();
    assert!(terminal.text().contains("end"), "{}", terminal.text());
    let text = terminal.text();
    let right: String = text
        .lines()
        .skip_while(|line| !line.contains("end"))
        .filter_map(|line| line.rsplit_once('│').map(|(_, value)| value.trim()))
        .collect();
    assert!(right.contains("value"), "{text}");
}

#[test]
fn setext_and_table_candidates_remain_local_until_disambiguated() {
    let (mut r, terminal) = renderer(30, 80);
    characters(&mut r, "Title\n===\n\n| H | V |\n|---|---|\n| x | y |\n");
    r.finish().unwrap();
    let text = terminal.text();
    assert_eq!(text.matches("Title").count(), 1, "{text}");
    assert!(!text.contains("==="));
    assert!(!text.contains("| x | y |"));
}

#[test]
fn mixed_document_output_is_independent_of_fragment_boundaries() {
    let source = include_str!("../../../tests/fixtures/markdown.md");
    let render = |chunk: usize| {
        let (mut r, terminal) = renderer(200, 80);
        let mut start = 0;
        while start < source.len() {
            let mut end = (start + chunk).min(source.len());
            while !source.is_char_boundary(end) {
                end += 1;
            }
            r.push(&source[start..end]).unwrap();
            start = end;
        }
        r.finish().unwrap();
        terminal.text()
    };
    let expected = render(source.len());
    for chunk in [1, 2, 3, 7, 31, 64] {
        assert_eq!(render(chunk), expected, "chunk={chunk}");
    }
}

#[test]
fn an_oversized_code_line_does_not_accidentally_close_its_fence() {
    let (mut r, terminal) = renderer(30, 80);
    r.options.max_pending_bytes = 64;
    r.push("```text\n").unwrap();
    r.push(&format!("{}x\n", "`".repeat(100))).unwrap();
    assert!(matches!(r.stack.leaf(), Some(Leaf::Code(_))));
    r.push("still code\n```\n").unwrap();
    r.finish().unwrap();
    assert_eq!(terminal.bg("still code", 2), vt100::Color::Rgb(35, 40, 48));
}

#[test]
fn oversized_html_lines_keep_their_terminator_across_chunks() {
    let (mut r, terminal) = renderer(30, 80);
    r.options.max_pending_bytes = 32;
    r.push("<!--\n").unwrap();
    r.push(&format!("{}-->tail\n", "x".repeat(63))).unwrap();
    r.push("# heading\n").unwrap();
    r.finish().unwrap();
    assert!(r.stack.leaf().is_none());
    assert!(terminal.text().contains("heading"));
    assert!(!terminal.text().contains("# heading"));
}

#[test]
fn active_code_does_not_call_the_markdown_parser_for_its_lines() {
    let (mut r, _) = renderer(8, 80);
    r.push("```rust\n").unwrap();
    let calls = crate::markdown::parser_calls();
    for n in 0..1000 {
        r.push(&format!("let value{n} = {n};\n")).unwrap();
    }
    assert_eq!(crate::markdown::parser_calls(), calls);
}

#[test]
fn tabs_in_list_markers_use_four_column_stops() {
    let (mut r, terminal) = renderer(30, 80);
    characters(&mut r, "1.\t```text\n\tbody\n\t```\n2.\tnext\n");
    r.finish().unwrap();
    let text = terminal.text();
    assert!(text.contains("body") && text.contains("2. next"), "{text}");
    assert_eq!(terminal.bg("body", 5), vt100::Color::Rgb(35, 40, 48));
}

#[path = "math_tests.rs"]
mod math_tests;
