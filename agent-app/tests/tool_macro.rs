use agent_core::{
    tool,
    tool::{Arguments, Registry},
};

#[tool]
fn add(left: i64, right: i64) -> i64 {
    left + right
}

#[tool(name = "welcome")]
fn greet(name: String) -> String {
    format!("你好，{name}")
}

#[tool(finish_session)]
fn finish(summary: String) -> String {
    summary
}

#[test]
fn registers_and_invokes_a_generated_tool() {
    let mut registry = Registry::new();
    registry.register(add_tool()).unwrap();
    registry.register(greet_tool()).unwrap();
    registry.register(finish_tool()).unwrap();

    let result = registry
        .invoke(
            "add",
            &Arguments::new().with("left", "2").with("right", "40"),
        )
        .unwrap();
    assert_eq!(result.content(), "42");

    let result = registry
        .invoke("welcome", &Arguments::new().with("name", "小凯"))
        .unwrap();
    assert_eq!(result.content(), "\"你好，小凯\"");

    let result = registry
        .invoke("finish", &Arguments::new().with("summary", "会话完成"))
        .unwrap();
    assert!(result.finishes_session());
    assert_eq!(result.content(), "会话完成");
}
