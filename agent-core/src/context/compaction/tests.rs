use super::*;
use crate::tool::{Arguments, ToolCall};

fn assert_protocol(context: &Context) {
    let mut pending = std::collections::BTreeSet::new();
    for message in context.messages() {
        if let Some(id) = message.tool_call_id() {
            assert!(pending.remove(id));
        } else {
            assert!(pending.is_empty());
            if let Some(calls) = message.tool_calls() {
                pending.extend(calls.iter().map(|c| c.id().to_owned()));
            }
        }
    }
    assert!(pending.is_empty());
}
#[test]
fn compaction_keeps_instructions_and_protocol_and_does_not_repeat_below_watermark() {
    let mut context = Context::with_history_limit(usize::MAX);
    context.push_user("never modify files");
    for i in 0..12 {
        context.push_assistant_with_tool_calls(
            "",
            vec![ToolCall::new(
                i.to_string(),
                "read",
                Arguments::new().with("path", "source.rs"),
            )],
        );
        context.push_tool(i.to_string(), "read", "evidence ".repeat(80));
        context.push_observation("rebuildable state");
    }
    let window = ContextWindow {
        high_bytes: 5000,
        low_bytes: 2000,
        max_messages: 50,
        summary_bytes: 512,
    };
    let info = context.compact(window).unwrap().unwrap();
    assert!(info.after_bytes < info.before_bytes);
    assert!(
        context
            .history()
            .any(|m| m.content() == "never modify files" && m.is_instruction())
    );
    assert_protocol(&context);
    let saved = context.snapshot();
    assert!(context.compact(window).unwrap().is_none());
    assert_eq!(context.snapshot(), saved);
    let restored: Context =
        serde_json::from_value(serde_json::to_value(&context).unwrap()).unwrap();
    assert_eq!(context, restored);
}
#[test]
fn oversized_complete_batch_is_archived_whole_and_large_instructions_are_not_truncated() {
    let mut context = Context::with_history_limit(usize::MAX);
    context.push_user("goal");
    context.push_assistant_with_tool_calls("", vec![ToolCall::new("a", "read", Arguments::new())]);
    context.push_tool("a", "read", "中".repeat(4000));
    let window = ContextWindow {
        high_bytes: 4000,
        low_bytes: 2000,
        max_messages: 16,
        summary_bytes: 512,
    };
    assert!(context.compact(window).unwrap().is_some());
    assert_protocol(&context);
    assert!(encoded_size(&context).unwrap() < window.high_bytes);
    context.clear_history();
    let instruction = "constraint ".repeat(2000);
    context.push_user(&instruction);
    assert!(context.compact(window).unwrap().is_none());
    assert_eq!(context.last().unwrap().content(), instruction);
}
