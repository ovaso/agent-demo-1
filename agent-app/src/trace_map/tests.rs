use super::*;
use serde_json::json;

fn event(name: &str, fields: Value) -> String {
    format!("{}\n", json!({"name": name, "fields": fields}))
}

fn start(run: &str, id: u64, parent: Option<u64>, operation: &str) -> String {
    event(
        "trace.span.started",
        json!({"run_id": run, "session_id": "same-session", "span_id": id, "parent_span_id": parent, "depth": u64::from(parent.is_some()), "operation": operation, "start_offset_ms": 1.25, "details": {}}),
    )
}

#[test]
fn orders_parents_before_children_and_separates_runs_in_one_session() {
    let input = start("r1", 1, None, "agent.run")
        + &start("r1", 2, Some(1), "model.request")
        + &event(
            "trace.span.completed",
            json!({"run_id":"r1", "span_id":2, "status":"ok", "duration_ms":2.5, "usage":{"input_tokens":10,"output_tokens":2}, "total_tokens":12, "cache_status":"unknown"}),
        )
        + &start("r2", 1, None, "agent.run")
        + &event(
            "model.first_text_delta",
            json!({"run_id":"r1", "span_id":2,"latency_ms":1}),
        );
    let mut output = Vec::new();
    render(input.as_bytes(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.find("agent.run").unwrap() < text.find("  model.request").unwrap());
    assert!(text.contains("token(in/out/total)=10/2/12 cache=unknown"));
    assert!(text.contains("首字=1ms"));
    assert_eq!(text.matches("incomplete").count(), 2);
    assert!(text.find("run=r1").unwrap() < text.find("run=r2").unwrap());
}

#[test]
fn tolerates_old_logs_and_partial_last_record_but_rejects_corruption() {
    let mut output = Vec::new();
    let input =
        event("agent.run.started", json!({})) + &start("r", 1, None, "agent.run") + "{\"name\":";
    render(input.as_bytes(), &mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("末尾不完整"));
    assert!(render(b"bad json\n".as_slice(), &mut Vec::new()).is_err());
    assert!(
        render(
            start("r", 2, Some(1), "tool.call").as_bytes(),
            &mut Vec::new()
        )
        .is_err()
    );
}

#[test]
fn bounds_line_size() {
    let input = vec![b' '; MAX_LINE_BYTES as usize + 1];
    assert!(
        render(input.as_slice(), &mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("4 MiB")
    );
}
