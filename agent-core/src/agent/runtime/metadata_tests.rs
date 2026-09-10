use super::{
    tests::{Memories, Model},
    *,
};
use crate::{
    context::Context,
    model::ModelResponse,
    tool::{Arguments, Parameter, Registry, Tool, ToolError, ToolOutput},
};

struct Versioned {
    name: &'static str,
    version: &'static str,
    description: &'static str,
}
impl Tool for Versioned {
    fn created_at(&self) -> u64 {
        100
    }
    fn name(&self) -> &str {
        self.name
    }
    fn version(&self) -> &str {
        self.version
    }
    fn description(&self) -> &str {
        self.description
    }
    fn parameters(&self) -> &[Parameter] {
        &[]
    }
    fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text("same behavior"))
    }
}

fn tools(version: &'static str, description: &'static str) -> Registry {
    let mut tools = Registry::new();
    tools
        .register(Versioned {
            name: "inspect",
            version,
            description,
        })
        .unwrap();
    tools
}

fn runtime() -> Runtime<Model, MemoryRunStore, Memories> {
    Runtime::new(
        Model(vec![Ok(ModelResponse::text("done"))].into()),
        MemoryRunStore::new(),
        Memories::default(),
        tools("v1.0.0", "inspect"),
    )
}

#[test]
fn legacy_checkpoints_refresh_metadata_without_granting_new_tools() {
    let mut runtime = runtime();
    let state = runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let mut old = serde_json::to_value(state).unwrap();
    for tool in old["tools"].as_array_mut().unwrap() {
        tool.as_object_mut().unwrap().remove("created_at");
        tool.as_object_mut().unwrap().remove("version");
    }
    let mut old: RunState = serde_json::from_value(old).unwrap();
    assert_eq!(old.tools[0].created_at(), 0);
    runtime.commit(&mut old).unwrap();
    runtime.tools = tools("v2.0.0-20260910", "inspect");
    runtime
        .tools
        .register(Versioned {
            name: "new_tool",
            version: "v1",
            description: "new",
        })
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.tools.len(), 1);
    assert_eq!(state.tools[0].name(), "inspect");
    assert_eq!(state.tools[0].created_at(), 100);
    assert_eq!(state.tools[0].version(), "v2.0.0-20260910");
    assert_eq!(state.result().unwrap().text(), "done");
    assert_eq!(runtime.state("run").unwrap().tools, state.tools);
}

#[test]
fn reference_versions_stay_out_of_model_views_and_do_not_relax_contracts() {
    let mut runtime = runtime();
    let mut state = runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let before = super::planning_view::message(&state).unwrap();
    state.tools[0] = state.tools[0].clone().with_metadata(100, "v2.0.0-20300101");
    let after = super::planning_view::message(&state).unwrap();
    assert_eq!(before.content(), after.content());
    assert!(!after.content().contains("created_at"));
    assert!(!after.content().contains("v2.0.0-20300101"));
    runtime.tools = tools("v2.0.0-20300101", "different contract");
    assert!(matches!(
        runtime.resume("run", &mut |_| {}),
        Err(RuntimeError::Invalid(_))
    ));
}
