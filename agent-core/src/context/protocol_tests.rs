use super::*;
use crate::tool::Arguments;

fn batch(context: &mut Context) {
    context.push_assistant_with_tool_calls(
        "",
        vec![
            ToolCall::new("a", "echo", Arguments::new()),
            ToolCall::new("b", "echo", Arguments::new()),
        ],
    );
}

#[test]
fn retains_an_active_tool_batch_even_when_the_message_limit_is_smaller() {
    let mut context = Context::with_history_limit(1);
    context.push_user("run");
    batch(&mut context);
    context.push_tool("a", "echo", "first");
    // Checkpointing halfway through a batch must not detach its results.
    let json = serde_json::to_string(&context).unwrap();
    let mut restored: Context = serde_json::from_str(&json).unwrap();
    restored.push_tool("b", "echo", "second");
    let messages = restored.snapshot();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].tool_calls().unwrap().len(), 2);
    assert_eq!(messages[1].tool_call_id(), Some("a"));
    assert_eq!(messages[2].tool_call_id(), Some("b"));
    restored.push_assistant("done");
    assert_eq!(restored.snapshot(), vec![Message::assistant("done")]);
}

#[test]
fn reducing_limit_evicts_old_exchanges_as_a_unit_and_preserves_system() {
    let mut context = Context::with_history_limit(20);
    context.set_system_prompt("system");
    batch(&mut context);
    context.push_tool("a", "echo", "first");
    context.push_tool("b", "echo", "second");
    context.push_assistant("done");
    context.push_user("next");
    context.set_history_limit(3);
    assert_eq!(
        context.snapshot(),
        vec![
            Message::system("system"),
            Message::assistant("done"),
            Message::user("next"),
        ]
    );
}
