//! 工具调用链路的回显探针。保留迁移前的名称、参数、权限和 JSON 输出格式。

use agent_core::tool;

#[tool(created_at = 1788784117, version = "v1.0.0-20260910", group = "debug")]
fn echo(text: String) -> String {
    text
}

#[cfg(test)]
mod tests {
    use agent_core::tool::{Arguments, Parameter, Registry, ToolDefinition};

    #[test]
    fn echo_is_debug_only_and_preserves_its_definition_and_output() {
        let mut ordinary = Registry::new();
        ordinary.register_group("default").unwrap();
        assert!(!ordinary.contains("echo"));

        let mut debug = Registry::new();
        crate::register(&mut debug, "RS_AGENT_MAX_STEPS=8\n".into()).unwrap();
        assert!(debug.contains("debug_show_config"));
        let definition = debug
            .definitions()
            .into_iter()
            .find(|tool| tool.name() == "echo")
            .unwrap();
        assert!(definition.same_contract(&ToolDefinition::new(
            "echo",
            "由函数 echo 自动生成的工具",
            vec![Parameter::required("text", "参数 text")],
        )));
        assert_eq!(definition.created_at(), 1788784117);
        assert!(!definition.version().is_empty());
        assert_eq!(
            debug
                .invoke("echo", &Arguments::new().with("text", "hello\nworld"))
                .unwrap()
                .content(),
            "\"hello\\nworld\""
        );
    }
}
