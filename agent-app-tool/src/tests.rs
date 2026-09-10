use agent_core::tool::{Registry, ToolDefinition};

#[test]
fn automatically_registered_tools_preserve_existing_checkpoint_definitions() {
    let expected: Vec<ToolDefinition> =
        serde_json::from_str(include_str!("tests/definitions.json")).unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let actual = registry.definitions();
    assert_eq!(actual.len(), expected.len());
    for legacy in expected {
        let tool = actual
            .iter()
            .find(|tool| tool.name() == legacy.name())
            .unwrap();
        assert!(tool.same_contract(&legacy));
        assert!(tool.created_at() > 0);
        assert!(!tool.version().trim().is_empty());
    }
    assert!(
        actual
            .windows(2)
            .all(|tools| tools[0].sort_key() < tools[1].sort_key())
    );
    assert!(!registry.contains("echo"));
    assert!(!registry.contains("debug_show_config"));
    assert!(!registry.contains("runtime_plan"));
}
