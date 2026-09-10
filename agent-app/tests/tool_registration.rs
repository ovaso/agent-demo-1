use agent_core::{
    tool,
    tool::{Arguments, Registry, RegistryError, ToolOutput},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Return literal text with an optional repeat count.
#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    read_only,
    output = "text"
)]
fn literal(#[arg(description = "Text to repeat")] text: String, count: Option<usize>) -> String {
    text.repeat(count.unwrap_or(1))
}

#[tool(created_at = 1789029900, version = "v1.0.0-20260910")]
#[cfg(any())]
fn disabled(_value: NotAType) -> bool {
    false
}

#[tool(created_at = 1789029900, version = "v1.0.0-20260910")]
#[cfg_attr(all(), cfg(any()))]
fn also_disabled(_value: NotAType) -> bool {
    false
}

struct State {
    label: String,
    calls: AtomicUsize,
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "state",
    output = "text"
)]
fn stateful(#[context] state: &State, suffix: Option<String>) -> String {
    state.calls.fetch_add(1, Ordering::Relaxed);
    format!("{}{}", state.label, suffix.unwrap_or_default())
}

mod another_file_scope {
    use super::*;

    #[tool(
        created_at = 1789029900,
        version = "v1.0.0-20260910",
        group = "state",
        read_only
    )]
    fn count(#[context] state: &State) -> usize {
        state.calls.load(Ordering::Relaxed)
    }

    #[tool(
        created_at = 1789029900,
        version = "v1.0.0-20260910",
        group = "state",
        read_only,
        output = "text"
    )]
    fn aaa_stateless() -> &'static str {
        "available"
    }
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "results",
    output = "tool"
)]
fn unsuccessful() -> Result<ToolOutput, &'static str> {
    Ok(ToolOutput::text("check failed").with_success(false))
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "results",
    output = "tool"
)]
fn completed() -> ToolOutput {
    ToolOutput::finish_session("finished")
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "results",
    output = "text"
)]
fn failure() -> Result<String, &'static str> {
    Err("test error")
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "results",
    finish_session,
    output = "text"
)]
fn finish_text() -> &'static str {
    "line one\nline two"
}

mod duplicates {
    use super::*;
    #[tool(
        created_at = 1789029900,
        version = "v1.0.0-20260910",
        group = "duplicates",
        name = "same"
    )]
    fn first() -> bool {
        true
    }
    #[tool(
        created_at = 1789029900,
        version = "v1.0.0-20260910",
        group = "duplicates",
        name = "same"
    )]
    fn second() -> bool {
        false
    }
}

#[test]
fn annotation_alone_discovers_tools_and_excludes_other_groups() {
    let mut registry = Registry::new();
    registry.register_group("default").unwrap();
    let definitions = registry.definitions();
    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.name(), "literal");
    assert_eq!(
        definition.description(),
        "Return literal text with an optional repeat count."
    );
    assert!(definition.is_read_only());
    assert_eq!(definition.parameters()[0].description(), "Text to repeat");
    assert!(definition.parameters()[0].is_required());
    assert!(!definition.parameters()[1].is_required());
    assert!(!registry.contains("stateful"));
    for text in ["123", "true", "null", "[1,2]", "first\nsecond"] {
        assert_eq!(
            registry
                .invoke("literal", &Arguments::new().with("text", text))
                .unwrap()
                .content(),
            text
        );
    }
    assert_eq!(
        registry
            .invoke(
                "literal",
                &Arguments::new()
                    .with("text", "\"quoted\"")
                    .with("count", "2")
            )
            .unwrap()
            .content(),
        "quotedquoted"
    );
    assert!(matches!(
        registry.invoke("literal", &Arguments::new()),
        Err(RegistryError::MissingArgument { .. })
    ));
    assert!(matches!(
        registry.invoke(
            "literal",
            &Arguments::new().with("text", "x").with("count", "bad")
        ),
        Err(RegistryError::Execution { .. })
    ));
    assert!(matches!(
        registry.invoke(
            "literal",
            &Arguments::new().with("text", "x").with("extra", "x")
        ),
        Err(RegistryError::UnexpectedArgument { .. })
    ));
}

#[test]
fn context_is_shared_by_tools_but_isolated_between_registries() {
    let context = Arc::new(State {
        label: "first".into(),
        calls: AtomicUsize::new(0),
    });
    let mut first = Registry::new();
    first
        .register_group_with_context("state", Arc::clone(&context))
        .unwrap();
    assert_eq!(
        first
            .definitions()
            .iter()
            .map(|tool| tool.name())
            .collect::<Vec<_>>(),
        ["aaa_stateless", "count", "stateful"]
    );
    assert!(
        first
            .definitions()
            .iter()
            .all(|tool| tool.parameters().iter().all(|p| p.name() != "state"))
    );
    assert_eq!(
        first
            .invoke("stateful", &Arguments::new().with("suffix", "!"))
            .unwrap()
            .content(),
        "first!"
    );
    assert_eq!(
        first.invoke("count", &Arguments::new()).unwrap().content(),
        "1"
    );
    assert_eq!(context.calls.load(Ordering::Relaxed), 1);
    assert!(matches!(
        first.invoke("stateful", &Arguments::new().with("state", "forged")),
        Err(RegistryError::UnexpectedArgument { .. })
    ));

    let mut second = Registry::new();
    second
        .register_group_with_context(
            "state",
            Arc::new(State {
                label: "second".into(),
                calls: AtomicUsize::new(0),
            }),
        )
        .unwrap();
    assert_eq!(
        second
            .invoke("stateful", &Arguments::new())
            .unwrap()
            .content(),
        "second"
    );
    assert_eq!(context.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn automatic_registration_is_atomic_on_missing_context_and_duplicates() {
    let mut registry = Registry::new();
    registry.register_group("default").unwrap();
    let before = registry.definitions();
    assert!(matches!(
        registry.register_group("state"),
        Err(RegistryError::MissingContext { .. })
    ));
    assert_eq!(registry.definitions(), before);
    assert!(matches!(
        registry.register_group_with_context("state", Arc::new("wrong type")),
        Err(RegistryError::MissingContext { .. })
    ));
    assert_eq!(registry.definitions(), before);
    assert!(matches!(
        registry.register_group("duplicates"),
        Err(RegistryError::DuplicateTool { .. })
    ));
    assert_eq!(registry.definitions(), before);
    assert!(matches!(
        registry.register_group("default"),
        Err(RegistryError::DuplicateTool { .. })
    ));
    assert_eq!(registry.definitions(), before);
    registry.register_group("unknown empty group").unwrap();
    assert_eq!(registry.definitions(), before);
}

#[test]
fn output_modes_preserve_errors_business_failure_and_session_completion() {
    let mut registry = Registry::new();
    registry.register_group("results").unwrap();
    let output = registry.invoke("unsuccessful", &Arguments::new()).unwrap();
    assert_eq!(output.content(), "check failed");
    assert!(!output.succeeded());
    assert!(!output.finishes_session());
    assert!(
        registry
            .invoke("completed", &Arguments::new())
            .unwrap()
            .finishes_session()
    );
    let text = registry.invoke("finish_text", &Arguments::new()).unwrap();
    assert!(text.finishes_session());
    assert_eq!(text.content(), "line one\nline two");
    let error = registry.invoke("failure", &Arguments::new()).unwrap_err();
    assert!(error.to_string().contains("test error"));
}

#[tool(
    created_at = 1789029900,
    version = "v1.0.0-20260910",
    group = "borrowed",
    output = "text"
)]
fn borrowed_text(text: &str, suffix: Option<&str>) -> String {
    format!("{text}{}", suffix.unwrap_or_default())
}

#[test]
fn borrowed_inputs_preserve_provider_decoded_text_and_optional_literals() {
    let mut registry = Registry::new();
    registry.register_group("borrowed").unwrap();
    for text in [
        "\"quoted\"",
        "null",
        "123",
        "true",
        "{\"key\":1}",
        "line\nnext",
        "",
        r"\n",
    ] {
        let arguments = Arguments::new().with("text", text);
        assert_eq!(
            registry
                .invoke("borrowed_text", &arguments)
                .unwrap()
                .content(),
            text
        );
        assert_eq!(
            registry
                .invoke("borrowed_text", &arguments.with("suffix", "null"))
                .unwrap()
                .content(),
            format!("{text}null")
        );
    }
    assert!(matches!(
        registry.invoke("borrowed_text", &Arguments::new()),
        Err(RegistryError::MissingArgument { .. })
    ));
}

#[tool(
    created_at = 100,
    name = "z_old",
    version = "v99.0.0-20260910",
    group = "ordered",
    output = "text"
)]
fn old_z() -> &'static str {
    "old"
}

#[tool(
    created_at = 100,
    name = "b_old",
    version = "v1.0.0",
    group = "ordered",
    output = "text"
)]
fn old_b() -> &'static str {
    "old"
}

#[tool(
    created_at = 200,
    name = "a_new",
    version = "v0.0.1",
    group = "ordered",
    output = "text"
)]
fn new_a() -> &'static str {
    "new"
}

#[test]
fn later_tools_append_to_the_schema_prefix_and_versions_do_not_sort() {
    let mut registry = Registry::new();
    registry.register(old_z_tool()).unwrap();
    registry.register(old_b_tool()).unwrap();
    let prefix = registry.definitions();
    assert_eq!(
        prefix.iter().map(|tool| tool.name()).collect::<Vec<_>>(),
        ["b_old", "z_old"]
    );
    registry.register(new_a_tool()).unwrap();
    let all = registry.definitions();
    assert_eq!(&all[..prefix.len()], prefix);
    assert_eq!(all[2].name(), "a_new");
    assert_eq!(all[2].created_at(), 200);
    assert_eq!(all[2].version(), "v0.0.1");
    let mut discovered = Registry::new();
    discovered.register_group("ordered").unwrap();
    assert_eq!(discovered.definitions(), all);
    let version_only = all[0].clone().with_metadata(100, "v500.0.0-20301231");
    assert_eq!(version_only.sort_key(), all[0].sort_key());
    assert!(version_only.same_contract(&all[0]));
}
