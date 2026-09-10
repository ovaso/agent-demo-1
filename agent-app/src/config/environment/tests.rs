use super::*;

fn parse(source: &str) -> io::Result<Environment> {
    Environment::parse(source, Path::new("fixture.env"))
}

#[test]
fn parses_quotes_comments_exports_multiline_and_file_references() {
    let name = "RS_AGENT_DOTENV_FIXTURE_VALUE";
    let before = env::var(name);
    let values = parse("export RS_AGENT_DOTENV_FIXTURE_VALUE=base\nJOINED=${RS_AGENT_DOTENV_FIXTURE_VALUE}/child\nQUOTED=\"a # b\"\nLITERAL='${literal}\\n'\nPLAIN=value # comment\nLINES=\"first\\nsecond\"\nMULTI='one\ntwo'\nEMPTY=\n").unwrap();
    let base = before.as_deref().unwrap_or("base");
    assert_eq!(values.file_values["JOINED"], format!("{base}/child"));
    assert_eq!(values.file_values["QUOTED"], "a # b");
    assert_eq!(values.file_values["LITERAL"], "${literal}\\n");
    assert_eq!(values.file_values["PLAIN"], "value");
    assert_eq!(values.file_values["LINES"], "first\nsecond");
    assert_eq!(values.file_values["MULTI"], "one\ntwo");
    assert_eq!(values.file_values["EMPTY"], "");
    assert_eq!(env::var(name), before, "parsing must not export values");
}

#[test]
fn process_values_including_empty_ones_take_precedence() {
    let values = parse("VALUE=file\n").unwrap();
    assert_eq!(
        values.resolve("VALUE", Ok("process".into())).unwrap(),
        "process"
    );
    assert_eq!(values.resolve("VALUE", Ok(String::new())).unwrap(), "");
    assert_eq!(
        values
            .resolve("VALUE", Err(env::VarError::NotPresent))
            .unwrap(),
        "file"
    );
    assert!(matches!(
        values.resolve("MISSING", Err(env::VarError::NotPresent)),
        Err(env::VarError::NotPresent)
    ));
}

#[test]
fn duplicate_keys_and_nul_are_rejected() {
    assert!(
        parse("A=first\nA=second\n")
            .err()
            .expect("invalid configuration")
            .to_string()
            .contains("重复")
    );
    assert!(parse("A=\"nul\0value\"\n").is_err());
}

#[test]
fn parse_errors_redact_values_and_report_the_file() {
    let error = parse("VALID=1\nOPENAI_API_KEY=\"sensitive-fixture-value\n")
        .err()
        .expect("invalid configuration")
        .to_string();
    assert!(error.contains("fixture.env"));
    assert!(!error.contains("sensitive-fixture-value"));
}

#[test]
fn parsed_configuration_has_count_and_size_limits() {
    let source: String = (0..=MAX_VARIABLES)
        .map(|index| format!("K{index}=x\n"))
        .collect();
    assert!(parse(&source).is_err());
    assert!(parse(&format!("VALUE={}\n", "x".repeat(MAX_FILE_BYTES))).is_err());
}
