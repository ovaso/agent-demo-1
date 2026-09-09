use std::collections::VecDeque;

use crate::{
    context::{ContextStore, MemoryContextStore, Role},
    memory::{MarkdownMemoryStore, MemoryStore},
    model::{ModelError, ModelRequest, ModelResponse, ModelUsage},
    tool::{Arguments, Parameter, Registry, Tool, ToolCall, ToolError, ToolOutput},
};

use super::*;

struct ScriptedModel {
    responses: VecDeque<ModelResponse>,
}

impl ScriptedModel {
    fn new(responses: impl IntoIterator<Item = ModelResponse>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
        }
    }
}

impl ModelProvider for ScriptedModel {
    fn complete(&mut self, _request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        self.responses
            .pop_front()
            .ok_or_else(|| ModelError::new("测试模型没有更多响应"))
    }
}

struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "返回输入文本"
    }

    fn parameters(&self) -> &[Parameter] {
        static PARAMETERS: [Parameter; 0] = [];
        &PARAMETERS
    }

    fn invoke(&self, _arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text("工具结果"))
    }
}

struct FinishTool;

impl Tool for FinishTool {
    fn name(&self) -> &str {
        "session_finish"
    }

    fn description(&self) -> &str {
        "结束会话"
    }

    fn parameters(&self) -> &[Parameter] {
        static PARAMETERS: [Parameter; 0] = [];
        &PARAMETERS
    }

    fn invoke(&self, _arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::finish_session("会话摘要"))
    }
}

#[test]
fn runs_a_tool_then_returns_the_final_text() {
    let model = ScriptedModel::new([
        ModelResponse::tool_calls(vec![ToolCall::new("call-1", "echo", Arguments::new())]),
        ModelResponse::text("最终回复"),
    ]);
    let mut tools = Registry::new();
    tools.register(EchoTool).unwrap();

    let directory =
        std::env::temp_dir().join(format!("rs-agent-runner-test-{}", std::process::id()));
    let memory_store = MarkdownMemoryStore::open(&directory).unwrap();
    let mut agent = Agent::new(model, MemoryContextStore::new(), memory_store, tools);

    let result = agent.run("session-1", "请调用工具").unwrap();

    assert_eq!(result.text(), "最终回复");
    assert_eq!(result.steps(), 2);
    let context = agent.context_store().load("session-1").unwrap().unwrap();
    assert_eq!(
        context
            .messages()
            .map(|message| message.role())
            .collect::<Vec<_>>(),
        vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant]
    );
    assert_eq!(
        context
            .messages()
            .nth(1)
            .unwrap()
            .tool_calls()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        context.messages().nth(2).unwrap().tool_call_id(),
        Some("call-1")
    );

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn forwards_text_deltas_while_running() {
    let model = ScriptedModel::new([ModelResponse::text("流式回复")]);
    let directory = std::env::temp_dir().join(format!(
        "rs-agent-stream-runner-test-{}",
        std::process::id()
    ));
    let memory_store = MarkdownMemoryStore::open(&directory).unwrap();
    let mut agent = Agent::new(
        model,
        MemoryContextStore::new(),
        memory_store,
        Registry::new(),
    );
    let mut output = String::new();

    let result = agent
        .run_stream("session-1", "你好", &mut |delta| output.push_str(delta))
        .unwrap();

    assert_eq!(result.text(), "流式回复");
    assert_eq!(output, "流式回复");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn finishes_the_session_and_preserves_a_summary_memory() {
    let model = ScriptedModel::new([ModelResponse::tool_calls(vec![ToolCall::new(
        "call-1",
        "session_finish",
        Arguments::new(),
    )])]);
    let mut tools = Registry::new();
    tools.register(FinishTool).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "rs-agent-finish-runner-test-{}",
        std::process::id()
    ));
    let memory_store = MarkdownMemoryStore::open(&directory).unwrap();
    let mut agent = Agent::new(model, MemoryContextStore::new(), memory_store, tools)
        .with_trace_sink(CollectedTrace::default());

    let result = agent.run("session-1", "今天就到这里").unwrap();

    assert_closed(&agent.trace_sink.0);
    assert_eq!(completed(&agent.trace_sink.0, "memory.save").len(), 1);
    assert_eq!(completed(&agent.trace_sink.0, "context.delete").len(), 1);
    assert!(result.session_finished());
    assert_eq!(result.text(), "会话摘要");
    assert_eq!(agent.context_store().load("session-1").unwrap(), None);
    assert_eq!(agent.memory_store().list().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[derive(Default)]
struct CollectedTrace(Vec<serde_json::Value>);

impl TraceSink for CollectedTrace {
    fn record(&mut self, event: crate::trace::TraceEvent) -> Result<(), TraceError> {
        self.0.push(serde_json::to_value(event).unwrap());
        Ok(())
    }
}

fn usage(input: u64, cached: Option<u64>) -> ModelUsage {
    ModelUsage {
        input_tokens: Some(input),
        output_tokens: Some(5),
        cached_input_tokens: cached,
        ..Default::default()
    }
}

fn completed<'a>(events: &'a [serde_json::Value], operation: &str) -> Vec<&'a serde_json::Value> {
    events
        .iter()
        .filter(|event| {
            event["name"] == "trace.span.completed" && event["fields"]["operation"] == operation
        })
        .map(|event| &event["fields"])
        .collect()
}

fn assert_closed(events: &[serde_json::Value]) {
    use std::collections::BTreeMap;
    let mut active = BTreeMap::new();
    for event in events {
        let fields = &event["fields"];
        let id = fields["span_id"].as_u64().unwrap();
        match event["name"].as_str().unwrap() {
            "trace.span.started" => {
                if let Some(parent) = fields["parent_span_id"].as_u64() {
                    assert!(active.contains_key(&parent));
                }
                assert!(active.insert(id, fields).is_none());
            }
            "trace.span.completed" | "trace.span.failed" => {
                let start = active.remove(&id).expect("span must start first");
                assert_eq!(start["operation"], fields["operation"]);
                assert!(fields["duration_ms"].as_f64().unwrap() >= 0.0);
            }
            _ => {}
        }
    }
    assert!(active.is_empty());
}

#[test]
fn traces_hierarchy_usage_and_separate_runs_with_recoverable_tool_errors() {
    let model = ScriptedModel::new([
        ModelResponse::tool_calls(vec![ToolCall::new("c1", "missing", Arguments::new())])
            .with_usage(usage(10, Some(0))),
        ModelResponse::text("recovered").with_usage(usage(20, Some(10))),
        ModelResponse::text("again").with_usage(usage(30, None)),
    ]);
    let directory =
        std::env::temp_dir().join(format!("rs-agent-trace-runs-{}", std::process::id()));
    let mut agent = Agent::new(
        model,
        MemoryContextStore::new(),
        MarkdownMemoryStore::open(&directory).unwrap(),
        Registry::new(),
    )
    .with_trace_sink(CollectedTrace::default());
    assert_eq!(agent.run("same", "go").unwrap().text(), "recovered");
    assert_closed(&agent.trace_sink.0);
    let root = completed(&agent.trace_sink.0, "agent.run")[0];
    assert_eq!(root["total_tokens"], 40);
    assert_eq!(root["model_calls"], 2);
    assert_eq!(root["cache_hits"], 1);
    assert_eq!(root["cache_reports"], 2);
    assert_eq!(completed(&agent.trace_sink.0, "agent.step").len(), 2);
    assert!(
        agent
            .trace_sink
            .0
            .iter()
            .any(|event| event["name"] == "trace.span.failed"
                && event["fields"]["operation"] == "tool.call")
    );
    agent.run("same", "next").unwrap();
    let roots = completed(&agent.trace_sink.0, "agent.run");
    assert_ne!(roots[0]["run_id"], roots[1]["run_id"]);
    assert_eq!(roots[1]["cache_status"], "unknown");
    assert!(roots[1]["usage"]["cached_input_tokens"].is_null());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn closes_ancestors_on_model_storage_empty_response_and_step_limit_errors() {
    for (name, responses, session, max_steps) in [
        ("model", vec![], "session", 1),
        (
            "empty",
            vec![ModelResponse::tool_calls(vec![])],
            "session",
            1,
        ),
        ("context", vec![], "", 1),
        (
            "limit",
            vec![ModelResponse::tool_calls(vec![ToolCall::new(
                "c",
                "missing",
                Arguments::new(),
            )])],
            "session",
            1,
        ),
        ("config", vec![], "session", 0),
    ] {
        let directory = std::env::temp_dir().join(format!(
            "rs-agent-trace-error-{name}-{}",
            std::process::id()
        ));
        let mut agent = Agent::new(
            ScriptedModel::new(responses),
            MemoryContextStore::new(),
            MarkdownMemoryStore::open(&directory).unwrap(),
            Registry::new(),
        )
        .with_config(AgentConfig::new(max_steps))
        .with_trace_sink(CollectedTrace::default());
        assert!(agent.run(session, "go").is_err());
        assert_closed(&agent.trace_sink.0);
        let root = agent.trace_sink.0.last().unwrap();
        assert_eq!(root["name"], "trace.span.failed");
        assert_eq!(root["fields"]["operation"], "agent.run");
        if name == "model" {
            assert_eq!(root["fields"]["model_calls"], 1);
            assert!(root["fields"]["total_tokens"].is_null());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
