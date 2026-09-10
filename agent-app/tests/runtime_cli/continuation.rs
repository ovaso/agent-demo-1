use super::support::{
    Fixture,
    http::{Provider, Step, call},
};
use serde_json::{Value, json};

#[test]
fn model_continuation_survives_checkpoint_and_provider_roundtrip() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let fixture = Fixture::new();
        std::fs::write(fixture.directory.join("source.txt"), "evidence").unwrap();
        fixture.run(
            provider,
            "/start inspect\n/step\n/exit\n",
            &[Step::tools(
                "main",
                vec![call("read", "read_file", json!({"path":"source.txt"}))],
            )
            .reasoning("synthetic continuation")],
        );
        let result = fixture.run(provider, "/resume\n/exit\n", &[Step::text("main", "done")]);
        let assistant = result.requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        match provider {
            Provider::OpenAi => {
                assert_eq!(assistant["reasoning_content"], "synthetic continuation")
            }
            Provider::Anthropic => {
                let blocks = assistant["content"].as_array().unwrap();
                assert_eq!(
                    blocks[0],
                    json!({"type":"thinking","thinking":"synthetic continuation","signature":"synthetic-signature"})
                );
                assert_eq!(blocks[1]["type"], "tool_use");
                assert_eq!(blocks[1]["input"]["path"], "source.txt");
            }
        }
        let events = std::fs::read_to_string(fixture.directory.join("trace.jsonl")).unwrap();
        assert!(
            events
                .lines()
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .any(|e| e["fields"]["response_model"] == "reported-model")
        );
    }
}
