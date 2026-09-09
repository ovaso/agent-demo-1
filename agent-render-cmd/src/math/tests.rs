use super::*;
fn options() -> Options {
    Options {
        color: true,
        ..Options::default()
    }
}
#[test]
fn strict_parser_preserves_math_instead_of_using_bareword_shortcuts() {
    assert_eq!(
        inline("a/b + pi", &options()).as_deref(),
        Some("a / b + pi")
    );
    assert_eq!(
        inline(r"\sqrt[3]{27}", &options()).as_deref(),
        Some("³√(27)")
    );
    assert_eq!(inline(r"E=mc^2", &options()).as_deref(), Some("E = mc²"));
    assert_eq!(inline(r"x_i^{n+1}", &options()).as_deref(), Some("xᵢⁿ⁺¹"));
    assert_eq!(
        inline(r"\frac{a+b}{c}", &options()).as_deref(),
        Some("(a + b)/(c)")
    );
}
#[test]
fn unknown_commands_missing_arguments_and_excessive_work_are_rejected() {
    for source in [
        r"\bad{x}",
        r"\frac{a}",
        "x^",
        "}",
        r"\sqrt{",
        r"\begin{matrix}1&2",
        r"\def\x{\x}\x",
        r"\newcommand{\x}{a}\x",
    ] {
        assert!(inline(source, &options()).is_none(), "{source}");
        assert!(display(source, 80, &options()).is_none(), "{source}");
    }
    assert!(inline(&"x".repeat(5000), &options()).is_none());
    assert!(
        inline(
            &format!("{}x{}", "{".repeat(40), "}".repeat(40)),
            &options()
        )
        .is_none()
    );
}
#[test]
fn display_math_has_two_dimensional_structure() {
    let text = display(r"\frac{a}{b}", 80, &options())
        .unwrap()
        .into_iter()
        .map(|l| l.plain.trim().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["a", "───", "b"]);
    let matrix = display(r"\begin{bmatrix}1&2\\3&4\end{bmatrix}", 80, &options()).unwrap();
    assert!(
        matrix
            .iter()
            .any(|l| l.plain.contains('1') && l.plain.contains('2'))
    );
    assert!(
        matrix
            .iter()
            .any(|l| l.plain.contains('3') && l.plain.contains('4'))
    );
    let cases = display(
        r"\begin{cases}x^2 & x>0 \\ 0 & x=0\end{cases}",
        80,
        &options(),
    )
    .unwrap();
    assert!(
        cases.iter().all(|l| !l.plain.contains("if")),
        "do not invent condition text"
    );
}
#[test]
fn disabled_math_and_overwide_display_choose_fallback() {
    let mut options = options();
    assert!(display(r"\frac{abcdef}{ghijkl}", 4, &options).is_none());
    options.render_math = false;
    assert!(inline("x^2", &options).is_none());
    assert!(display("x^2", 80, &options).is_none());
}
