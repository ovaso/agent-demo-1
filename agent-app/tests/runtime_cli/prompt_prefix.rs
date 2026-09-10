use super::support::{
    Fixture,
    http::{Provider, Step, call},
};
use serde_json::json;

#[test]
fn ordinary_tool_rounds_preserve_previous_request_prefix() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let fixture = Fixture::new();
        std::fs::write(fixture.directory.join("a.txt"), "alpha").unwrap();
        let read = |id| Step::tools("main", vec![call(id, "read_file", json!({"path":"a.txt"}))]);
        let result = fixture.run(
            provider,
            "inspect\n/exit\n",
            &[read("a"), read("b"), Step::text("main", "done")],
        );
        for pair in result.requests.windows(2) {
            let previous = pair[0]["messages"].as_array().unwrap();
            let next = pair[1]["messages"].as_array().unwrap();
            assert!(
                next.starts_with(previous),
                "previous model input was rewritten"
            );
            assert_eq!(pair[0]["tools"], pair[1]["tools"]);
        }
    }
}

#[test]
fn resumed_and_failed_requests_keep_their_prepared_prefix() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        for interrupted in [false, true] {
            let fixture = Fixture::new();
            std::fs::write(fixture.directory.join("a.txt"), "alpha").unwrap();
            let step = Step::tools(
                "main",
                vec![call("a", "read_file", json!({"path":"a.txt"}))],
            );
            let first = if interrupted {
                fixture.run(provider, "inspect\n/exit\n", &[step.interrupted()])
            } else {
                fixture.run(provider, "/start inspect\n/step\n/exit\n", &[step])
            };
            let next = fixture.run(provider, "/resume\n/exit\n", &[Step::text("main", "done")]);
            assert!(
                next.requests[0]["messages"]
                    .as_array()
                    .unwrap()
                    .starts_with(first.requests[0]["messages"].as_array().unwrap())
            );
        }
    }
}

#[test]
fn new_memory_matches_append_without_rewriting_the_previous_question() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let fixture = Fixture::new();
        let mut store =
            agent_core::memory::MarkdownMemoryStore::open(fixture.directory.join("memories"))
                .unwrap();
        use agent_core::memory::{Memory, MemoryStore};
        store.save(Memory::new("a", "alpha evidence")).unwrap();
        store.save(Memory::new("b", "beta evidence")).unwrap();
        let result = fixture.run(
            provider,
            "alpha\nbeta\n/exit\n",
            &[
                Step::text("main", "alpha done"),
                Step::text("main", "beta done"),
            ],
        );
        assert!(
            result.requests[1]["messages"]
                .as_array()
                .unwrap()
                .starts_with(result.requests[0]["messages"].as_array().unwrap())
        );
    }
}
