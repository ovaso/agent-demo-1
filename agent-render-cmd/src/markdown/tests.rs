use super::*;

fn options() -> Options {
    Options {
        color: true,
        ..Options::default()
    }
}
fn plain(source: &str) -> String {
    render(source, &options())
        .into_iter()
        .map(|l| l.plain)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nested_lists_and_continuations_keep_their_indentation() {
    assert_eq!(
        plain("- parent\n  - child\n    continuation\n  - sibling\n- next"),
        "  • parent\n    • child continuation\n    • sibling\n  • next"
    );
    assert_eq!(
        plain("9. one\n10. two\n    - inner"),
        "  9. one\n  10. two\n      • inner"
    );
}

#[test]
fn quoted_lists_preserve_both_prefixes() {
    assert_eq!(
        plain("> quoted\n>\n> - one\n>   - two"),
        "  │ quoted\n  │ • one\n  │   • two"
    );
}

#[test]
fn multiline_emphasis_and_code_are_distinct() {
    let lines = render("**first\nsecond**", &options());
    assert_eq!(lines[0].plain, "  first second");
    assert!(lines[0].ansi.contains("\x1b[1mfirst second"));
    let code = render("````rust\n**literal**\n```\n````", &options());
    assert_eq!(
        code.iter()
            .map(|line| line.plain.trim())
            .collect::<Vec<_>>(),
        ["rust", "**literal**", "```", ""]
    );
}

#[test]
fn sub_sup_preserve_inline_styles_and_support_case_insensitive_html_tags() {
    let source = "**x<SUP title=\"power\">2</SUP>** / *H<sub>2</sub>O* / a<sub>i-1</sub> / b<sup>(n+1)</sup>";
    let lines = render(source, &options());
    assert_eq!(lines[0].plain, "  x² / H₂O / aᵢ₋₁ / b⁽ⁿ⁺¹⁾");
    let mut terminal = vt100::Parser::new(10, 100, 0);
    terminal.process(lines[0].ansi.as_bytes());
    assert!(terminal.screen().cell(0, 3).unwrap().bold());
    assert!(terminal.screen().cell(0, 8).unwrap().italic());
}

#[test]
fn sub_sup_use_available_letter_forms_for_ordinals_and_indices() {
    assert_eq!(
        plain(
            "B<sup>j</sup> / K<sub>ij</sub><sup>*T*</sup> / 1<sup>st</sup> 2<sup>nd</sup> 3<sup>rd</sup> 4<sup>th</sup> / z<sup>~~old~~</sup>"
        ),
        "  Bʲ / Kᵢⱼᵀ / 1ˢᵗ 2ⁿᵈ 3ʳᵈ 4ᵗʰ / zᵒˡᵈ"
    );
    assert_eq!(
        plain("x<sub>bCd</sub> / x<sup>qXYZ</sup>"),
        "  x_{bCd} / x^{qXYZ}"
    );
}

#[test]
fn sub_sup_fallback_preserves_content_and_link_destinations() {
    assert_eq!(
        plain("中<sub>下标</sub> / 中<sup>上标</sup> / x<sub>i<sup>2</sup></sub>"),
        "  中_{下标} / 中^{上标} / x_{i²}"
    );
    assert_eq!(
        plain("x<sup>[2](https://a2.com)</sup> / [H<sub>2</sub>O](https://h2o.example)"),
        "  x^{2 (https://a2.com)} / H₂O (https://h2o.example)"
    );
}

#[test]
fn code_escaped_html_and_unknown_tags_stay_literal() {
    let text = plain(
        "`<sup>2</sup>` / &lt;sub&gt;2&lt;/sub&gt; / <supply>2</supply>\n\n```html\n<sup>2</sup>\n```\n",
    );
    assert_eq!(text.matches("<sup>2</sup>").count(), 2, "{text}");
    assert!(
        text.contains("<sub>2</sub>") && text.contains("<supply>2</supply>"),
        "{text}"
    );
    assert!(!text.contains('²') && !text.contains('₂'), "{text}");
}

#[test]
fn unmatched_sub_sup_cannot_spread_into_other_paragraphs_or_cells() {
    let text = plain("x<sub>2\n\nnext 3</sub>\n\n| a | b |\n|---|---|\n| x<sup>2 | 3</sup> |\n");
    assert!(
        text.contains("x<sub>2") && text.contains("next 3</sub>"),
        "{text}"
    );
    assert!(
        text.contains("x<sup>2") && text.contains("3</sup>"),
        "{text}"
    );
    assert!(!text.contains('²') && !text.contains('₂'), "{text}");
}

#[test]
fn blank_code_lines_keep_the_panel_background() {
    let lines = render("```python\nx = 1\n\nprint(x)\n```\n", &options());
    assert_eq!(
        lines
            .iter()
            .map(|line| line.plain.trim())
            .collect::<Vec<_>>(),
        ["python", "x = 1", "", "print(x)", ""]
    );
    assert!(lines.iter().all(|line| line.width == lines[0].width));
    assert!(lines[2].ansi.contains("\x1b[48;2;35;40;48m"));
    assert!(lines[4].ansi.contains("\x1b[48;2;35;40;48m"));
}

#[test]
fn code_panels_in_tight_lists_start_on_their_own_line() {
    let source = "- Install\n  ```bash\n  pip install example\n  ```\n- Next";
    let lines = render(source, &options());
    assert_eq!(lines[0].plain, "  • Install");
    assert_eq!(lines[1].plain.trim(), "bash");
    assert!(lines[2].plain.starts_with("      pip install example"));
    assert_eq!(lines.last().unwrap().plain, "  • Next");
}

#[test]
fn a_loose_list_remains_one_mutable_root() {
    assert!(plain("- first\n\n  more\n\n- second").contains("    more"));
}

#[test]
fn table_columns_align_with_chinese_emoji_and_combining_characters() {
    let source = "| 名称 | 状态 |\n| :--- | ---: |\n| 苹果 | ✅ |\n| e\u{301} | 好 |";
    let lines = render(source, &options());
    let widths: Vec<_> = lines.iter().map(|l| l.width).collect();
    assert!(widths.iter().all(|width| *width == widths[0]), "{widths:?}");
    assert!(lines.iter().any(|l| l.plain.contains('┼')));
    assert!(lines.iter().any(|l| l.plain.contains("苹果")));
    assert!(!lines.iter().any(|l| l.plain.contains("---")));
}

#[test]
fn narrow_tables_wrap_without_losing_cell_text() {
    let options = Options {
        columns: 16,
        ..options()
    };
    let lines = render(
        "| Key | Value |\n|---|---|\n| A | abcdefghijklmnop |",
        &options,
    );
    assert!(lines.iter().all(|l| l.width <= options.columns));
    let text: String = lines
        .iter()
        .flat_map(|l| l.plain.chars())
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    assert!(text.contains("abcdefghijklmnop"), "{text}");
}

#[test]
fn tiny_terminal_uses_stacked_table_cells_instead_of_truncation() {
    let options = Options {
        columns: 6,
        ..options()
    };
    let lines = render("| A | B | C |\n|---|---|---|\n| x | y | z |", &options);
    assert!(lines.iter().all(|l| l.width <= options.columns));
    let text = lines.iter().map(|l| l.plain.as_str()).collect::<String>();
    assert!(text.contains('x') && text.contains('y') && text.contains('z'));
}

#[test]
fn embedded_terminal_controls_are_printed_as_text() {
    let lines = render("hello\x1b[2J\n", &options());
    assert!(lines[0].plain.contains("�[2J"));
    assert!(!lines[0].ansi.contains("\x1b[2J"));
}

#[test]
fn grapheme_wrapping_does_not_split_combining_sequences() {
    let options = Options {
        columns: 8,
        ..options()
    };
    let lines = render("中e\u{301}文e\u{301}文", &options);
    assert!(lines.iter().all(|l| l.width <= 8));
    assert_eq!(
        lines.iter().map(|l| l.plain.trim()).collect::<String>(),
        "中e\u{301}文e\u{301}文"
    );
}

#[test]
fn automatic_links_show_the_address_only_once() {
    assert_eq!(plain("<https://example.com>"), "  https://example.com");
    assert_eq!(plain("<test@example.com>"), "  test@example.com");
}

#[test]
fn table_autolinks_show_the_address_only_once() {
    let rendered = plain("| Link |\n|---|\n| <https://example.com> |\n");
    assert_eq!(
        rendered.matches("https://example.com").count(),
        1,
        "{rendered}"
    );
}

#[test]
fn named_links_keep_destinations_but_equal_formatted_labels_do_not_repeat() {
    assert_eq!(
        plain("[文档](https://example.com)"),
        "  文档 (https://example.com)"
    );
    assert_eq!(
        plain("[https://**example**.com](https://example.com)"),
        "  https://example.com"
    );
    assert_eq!(
        plain("[test@example.com](mailto:test@example.com)"),
        "  test@example.com"
    );
    assert_eq!(
        plain("![示例图片](https://example.com/image.png)"),
        "  示例图片 (https://example.com/image.png)"
    );
}

#[test]
fn table_links_use_the_same_display_order_and_deduplication_as_paragraphs() {
    let rendered =
        plain("| 名称 |\n|---|\n| [文档](https://example.com) |\n| <test@example.com> |\n");
    assert!(rendered.contains("文档 (https://example.com)"));
    assert_eq!(rendered.matches("test@example.com").count(), 1);
}
