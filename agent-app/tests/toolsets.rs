use agent_core::tool::Registry;

#[test]
fn toolset_registration_order_does_not_change_serialized_definitions() {
    let mut ordinary_first = Registry::new();
    agent_app_tool::register(&mut ordinary_first).unwrap();
    agent_tool_debug::register(&mut ordinary_first, "test configuration".into()).unwrap();

    let mut debug_first = Registry::new();
    agent_tool_debug::register(&mut debug_first, "test configuration".into()).unwrap();
    agent_app_tool::register(&mut debug_first).unwrap();

    let definitions = ordinary_first.definitions();
    assert_eq!(definitions.len(), 9);
    assert!(
        definitions
            .windows(2)
            .all(|tools| tools[0].sort_key() < tools[1].sort_key())
    );
    assert_eq!(
        serde_json::to_string(&definitions).unwrap(),
        serde_json::to_string(&debug_first.definitions()).unwrap(),
    );
}
