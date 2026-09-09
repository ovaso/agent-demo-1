use super::*;

#[cfg(feature = "math")]
#[test]
fn inline_display_and_nested_math_are_independent_of_fragments() {
    let source = include_str!("../../../tests/fixtures/math.md");
    let mut offsets: Vec<_> = source.char_indices().map(|(i, _)| i).collect();
    offsets.push(source.len());
    let (mut r, expected) = renderer(140, 100);
    r.push(source).unwrap();
    r.finish().unwrap();
    let expected = expected.0.borrow().screen().contents_formatted();
    for chunk in [1, 2, 7, 31] {
        let (mut r, t) = renderer(140, 100);
        for start in (0..offsets.len() - 1).step_by(chunk) {
            r.push(&source[offsets[start]..offsets[(start + chunk).min(offsets.len() - 1)]])
                .unwrap();
        }
        r.finish().unwrap();
        assert_eq!(
            t.0.borrow().screen().contents_formatted(),
            expected,
            "chunk={chunk}"
        );
    }
}
#[cfg(feature = "math")]
#[test]
fn math_stays_on_stack_and_previews_source_until_closed() {
    let (mut r, t) = renderer(30, 80);
    r.push("$$\n").unwrap();
    let before = crate::markdown::parser_calls();
    characters(&mut r, r"\frac{a}{b}");
    assert!(matches!(r.stack.leaf(), Some(Leaf::Math(_))));
    assert!(t.text().contains(r"\frac{a}{b}"));
    assert_eq!(crate::markdown::parser_calls(), before);
    r.push("\n$$\n\n# after\n").unwrap();
    r.finish().unwrap();
    let text = t.text();
    assert!(text.contains("───") && !text.contains(r"\frac"), "{text}");
    assert!(
        text.contains("after") && !text.contains("# after"),
        "{text}"
    );
}
#[test]
fn unclosed_invalid_and_disabled_math_preserve_original_notation() {
    for source in [
        "$$\n\\frac{a}{b}",
        "$$\n\\bad{x}\n$$\n",
        "Inline $\\frac{a}$ and `$x^2$`.\n",
    ] {
        let (mut r, t) = renderer(30, 80);
        characters(&mut r, source);
        r.finish().unwrap();
        let text = t.text();
        assert!(text.contains("\\frac") || text.contains("\\bad"), "{text}");
        assert!(text.contains('$'), "{text}");
    }
    let (mut r, t) = renderer(30, 80);
    r.options.render_math = false;
    r.output.options = r.options.clone();
    characters(&mut r, "$$\nx^2\n$$\n\nInline $x^2$.\n");
    r.finish().unwrap();
    let text = t.text();
    assert!(text.contains("$$") && text.contains("$x^2$"), "{text}");
}
#[test]
fn long_math_falls_back_without_dropping_state_or_late_lines() {
    let terminal = Terminal::new(200, 80);
    let options = Options {
        color: true,
        rows: 8,
        columns: 80,
        max_pending_bytes: 64,
        ..Options::default()
    };
    let mut r = Renderer::new(terminal.clone(), options);
    r.push("$$\n").unwrap();
    for n in 0..100 {
        r.push(&format!("value{n:03} +\n")).unwrap();
        assert!(matches!(r.stack.leaf(),Some(Leaf::Math(m))if m.source.len()<=64));
    }
    r.push("$$\n\n# after\n").unwrap();
    r.finish().unwrap();
    let text = terminal.text();
    for n in 0..100 {
        assert_eq!(
            text.matches(&format!("value{n:03}")).count(),
            1,
            "n={n}\n{text}"
        );
    }
    assert!(
        text.contains("after") && !text.contains("# after"),
        "{text}"
    );
}
#[test]
fn math_resize_keeps_unfinished_source_without_duplication() {
    let (mut r, t) = renderer(24, 80);
    characters(&mut r, "$$\n\\frac{a");
    t.resize(24, 60);
    r.resize(60, 24).unwrap();
    characters(&mut r, "}{b}\n$$\n\n# after\n");
    r.finish().unwrap();
    let text = t.text();
    let flat = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    assert!(flat.contains(r"$$\frac{a}{b}$$"), "{text}");
    assert_eq!(text.matches(r"\frac").count(), 1, "{text}");
    assert!(text.contains("after") && !text.contains("# after"));
}
#[cfg(feature = "math")]
#[test]
fn bracket_blocks_inline_code_currency_and_math_tail_are_distinct() {
    let (mut r, t) = renderer(30, 80);
    characters(
        &mut r,
        "\\[\n\\frac{a}{b}\n\\]\n\n$$x^2$$ tail\n\n`$x^2$` and \\$5; $5 and $10.\n",
    );
    r.finish().unwrap();
    let text = t.text();
    assert!(
        text.contains("───") && text.contains("x²") && text.contains("tail"),
        "{text}"
    );
    assert!(
        text.contains("$x^2$") && text.contains("$5 and $10"),
        "{text}"
    );
}
