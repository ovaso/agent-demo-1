use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::{Agent, AgentError, AgentResult};
use crate::{
    context::{Context, ContextStore},
    memory::{Memory, MemoryStore},
    model::{ModelProvider, ModelRequest},
    trace::{TraceEvent, TraceSink},
};

pub(super) fn run<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    run_inner(agent, session_id, input, None)
}

pub(super) fn run_stream<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
    on_text_delta: &mut dyn FnMut(&str),
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    run_inner(agent, session_id, input, Some(on_text_delta))
}

fn run_inner<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
    mut on_text_delta: Option<&mut dyn FnMut(&str)>,
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    if agent.config.max_steps == 0 {
        return Err(AgentError::InvalidConfiguration(
            "max_steps 必须大于 0".to_owned(),
        ));
    }

    agent.trace_sink.record(
        TraceEvent::new("agent.run.started")
            .with_field("session_id", session_id)
            .with_field("input", input.clone()),
    )?;
    let mut context = agent
        .context_store
        .load(session_id)?
        .unwrap_or_else(Context::new);
    context.push_user(input.clone());
    agent.context_store.save(session_id, &context)?;

    let memories = agent.memory_store.search(&input)?;
    agent.trace_sink.record(
        TraceEvent::new("memory.search.completed")
            .with_field("session_id", session_id)
            .with_field("result_count", memories.len() as u64),
    )?;
    let tools = agent.tools.definitions();

    for step in 1..=agent.config.max_steps {
        let request = ModelRequest::new(context.snapshot(), &memories, &tools);
        agent.trace_sink.record(
            TraceEvent::new("model.request.started")
                .with_field("session_id", session_id)
                .with_field("loop_step", step as u64)
                .with_field("message_count", request.messages().len() as u64)
                .with_field("tool_count", request.tools().len() as u64),
        )?;

        let model_started = Instant::now();
        let mut first_delta_at = None;
        let mut emitted_characters = 0usize;
        let response = {
            let mut emit = |delta: &str| {
                if first_delta_at.is_none() {
                    first_delta_at = Some(Instant::now());
                }
                emitted_characters += delta.chars().count();
                if let Some(callback) = on_text_delta.as_deref_mut() {
                    callback(delta);
                }
            };
            agent.model.stream(request, &mut emit)?
        };
        let model_elapsed = model_started.elapsed();
        agent.trace_sink.record(
            TraceEvent::new("model.response.completed")
                .with_field("session_id", session_id)
                .with_field("loop_step", step as u64)
                .with_field("duration_ms", model_elapsed.as_millis() as u64)
                .with_field("emitted_characters", emitted_characters as u64)
                .with_field(
                    "characters_per_second",
                    if model_elapsed.is_zero() {
                        0.0
                    } else {
                        emitted_characters as f64 / model_elapsed.as_secs_f64()
                    },
                ),
        )?;
        if let Some(first_delta_at) = first_delta_at {
            agent.trace_sink.record(
                TraceEvent::new("model.first_text_delta")
                    .with_field("session_id", session_id)
                    .with_field("loop_step", step as u64)
                    .with_field(
                        "latency_ms",
                        first_delta_at.duration_since(model_started).as_millis() as u64,
                    ),
            )?;
        }
        let (text, calls) = response.into_parts();

        if calls.is_empty() {
            let text = text.ok_or(AgentError::EmptyModelResponse)?;
            context.push_assistant(&text);
            agent.context_store.save(session_id, &context)?;
            agent.trace_sink.record(
                TraceEvent::new("agent.run.completed")
                    .with_field("session_id", session_id)
                    .with_field("loop_steps", step as u64)
                    .with_field("output", text.clone()),
            )?;

            return Ok(AgentResult {
                text,
                steps: step,
                session_finished: false,
            });
        }

        context.push_assistant_with_tool_calls(text.unwrap_or_default(), calls.clone());

        for call in calls {
            agent.trace_sink.record(
                TraceEvent::new("tool.call.started")
                    .with_field("session_id", session_id)
                    .with_field("loop_step", step as u64)
                    .with_field("call_id", call.id())
                    .with_field("tool_name", call.name())
                    .with_field("arguments", arguments_value(call.arguments())),
            )?;
            match agent.tools.invoke(call.name(), call.arguments()) {
                Ok(output) => {
                    agent.trace_sink.record(
                        TraceEvent::new("tool.call.completed")
                            .with_field("session_id", session_id)
                            .with_field("call_id", call.id())
                            .with_field("tool_name", call.name())
                            .with_field("output", output.content()),
                    )?;
                    if output.finishes_session() {
                        let summary = output.content().to_owned();
                        context.push_tool(call.id(), call.name(), &summary);
                        let memory_id = format!(
                            "session-summary:{session_id}:{}",
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                        );
                        agent
                            .memory_store
                            .save(Memory::new(memory_id, &summary).with_tag("session-summary"))?;
                        agent.context_store.delete(session_id)?;
                        agent.trace_sink.record(
                            TraceEvent::new("agent.session.finished")
                                .with_field("session_id", session_id)
                                .with_field("loop_steps", step as u64)
                                .with_field("summary", summary.clone()),
                        )?;

                        return Ok(AgentResult {
                            text: summary,
                            steps: step,
                            session_finished: true,
                        });
                    }
                    context.push_tool(call.id(), call.name(), output.content())
                }
                Err(error) => {
                    agent.trace_sink.record(
                        TraceEvent::new("tool.call.failed")
                            .with_field("session_id", session_id)
                            .with_field("call_id", call.id())
                            .with_field("tool_name", call.name())
                            .with_field("error", error.to_string()),
                    )?;
                    context.push_tool(call.id(), call.name(), format!("工具调用失败：{error}"));
                }
            }
        }

        agent.context_store.save(session_id, &context)?;
    }

    Err(AgentError::MaxStepsExceeded {
        max_steps: agent.config.max_steps,
    })
}

fn arguments_value(arguments: &crate::tool::Arguments) -> Value {
    json!(
        arguments
            .iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::{
        context::{ContextStore, MemoryContextStore, Role},
        memory::{MarkdownMemoryStore, MemoryStore},
        model::{ModelError, ModelResponse},
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
        let mut agent = Agent::new(model, MemoryContextStore::new(), memory_store, tools);

        let result = agent.run("session-1", "今天就到这里").unwrap();

        assert!(result.session_finished());
        assert_eq!(result.text(), "会话摘要");
        assert_eq!(agent.context_store().load("session-1").unwrap(), None);
        assert_eq!(agent.memory_store().list().unwrap().len(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
