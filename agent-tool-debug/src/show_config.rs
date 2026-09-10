use agent_core::tool;

use super::DebugContext;

/// 显示本次启动实际生效的配置项（KEY=VALUE），包含默认值，不含 API 密钥。
/// 配置为启动快照，不反映运行中修改的环境文件或任务预算。
#[tool(
    created_at = 1789028730,
    version = "v1.0.0-20260910",
    group = "debug",
    read_only,
    output = "text"
)]
fn debug_show_config(#[context] context: &DebugContext) -> &str {
    &context.config_snapshot
}

#[cfg(test)]
mod tests {
    use agent_core::tool::{Arguments, Registry, RegistryError};

    #[test]
    fn registers_read_only_snapshot_and_rejects_arguments() {
        let mut registry = Registry::new();
        crate::register(&mut registry, "RS_AGENT_MAX_STEPS=8\n".into()).unwrap();
        let definition = registry
            .definitions()
            .into_iter()
            .find(|tool| tool.name() == "debug_show_config")
            .unwrap();
        assert!(definition.is_read_only());
        assert!(definition.parameters().is_empty());
        assert_eq!(
            registry
                .invoke("debug_show_config", &Arguments::new())
                .unwrap()
                .content(),
            "RS_AGENT_MAX_STEPS=8\n"
        );
        assert!(matches!(
            registry.invoke(
                "debug_show_config",
                &Arguments::new().with("key", "OPENAI_API_KEY")
            ),
            Err(RegistryError::UnexpectedArgument { .. })
        ));
    }
}
