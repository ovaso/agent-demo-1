//! Fixed local workloads; no network, provider variability, or new dependencies.
mod allocation;

use agent_core::{
    agent::{
        planning::{Plan, PlanTask, TaskAction},
        runtime::{MemoryRunStore, RunLimits, RunOptions, RunStore, Runtime},
    },
    context::Context,
    memory::{MarkdownMemoryStore, Memory, MemoryStore},
    model::{ModelError, ModelProvider, ModelRequest, ModelResponse},
    tool::{Arguments, Parameter, Registry, Tool, ToolCall, ToolError, ToolOutput},
};
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

#[global_allocator]
static ALLOCATOR: allocation::CountingAllocator = allocation::CountingAllocator;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("rs-agent-bench-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixedModel(usize);
impl ModelProvider for FixedModel {
    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        black_box((request.messages(), request.tools()));
        if self.0 == 0 {
            return Ok(ModelResponse::text("done"));
        }
        self.0 -= 1;
        Ok(ModelResponse::tool_calls(vec![ToolCall::new(
            format!("call-{}", self.0),
            "blob-0",
            Arguments::new(),
        )]))
    }
}

struct Blob {
    name: String,
    content: String,
}
impl Tool for Blob {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Return a fixed local payload for deterministic runtime measurements."
    }
    fn parameters(&self) -> &[Parameter] {
        &[]
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text(self.content.clone()))
    }
}

fn run(store: impl RunStore, directory: &Directory, id: usize, planning: bool) {
    let mut tools = Registry::new();
    for index in 0..16 {
        tools
            .register(Blob {
                name: format!("blob-{index}"),
                content: "x".repeat(16 * 1024),
            })
            .unwrap();
    }
    let memory = MarkdownMemoryStore::open(directory.0.join("empty-memories")).unwrap();
    let mut runtime = Runtime::new(FixedModel(16), store, memory, tools);
    let id = format!("run-{id}");
    runtime
        .start_with_options(
            &id,
            &id,
            "benchmark",
            Context::with_history_limit(16),
            RunOptions {
                limits: RunLimits::new(17),
                planning,
                ..Default::default()
            },
        )
        .unwrap();
    if planning {
        runtime
            .propose_plan(
                &id,
                0,
                Plan {
                    goal: "benchmark".into(),
                    requirements: vec!["fixed workload".into()],
                    tasks: (0..32)
                        .map(|index| PlanTask {
                            id: format!("task-{index}"),
                            description: "inspect fixed inputs ".repeat(12),
                            depends_on: vec![],
                            acceptance: vec!["report evidence".into()],
                            action: TaskAction::Agent {
                                prompt: "inspect and report ".repeat(12),
                            },
                        })
                        .collect(),
                },
            )
            .unwrap();
    }
    let done = runtime.resume(&id, &mut |_| {}).unwrap();
    assert_eq!(done.budget().model_calls(), 17);
    assert_eq!(done.budget().tool_calls(), 16);
    assert_eq!(done.result().unwrap().text(), "done");
    black_box(done);
}

fn measure(name: &str, iterations: usize, mut work: impl FnMut(usize)) {
    // Setup and the initial filesystem/cache warmup are excluded.
    work(0);
    let sample = allocation::Sample::begin();
    let start = Instant::now();
    for index in 1..=iterations {
        work(index);
    }
    let micros = start.elapsed().as_micros();
    let (calls, bytes, peak) = sample.finish();
    println!(
        "{{\"case\":\"{name}\",\"iterations\":{iterations},\"micros_per_iteration\":{},\"allocations_per_iteration\":{},\"allocated_bytes_per_iteration\":{},\"peak_additional_rust_heap_bytes\":{peak}}}",
        micros / iterations as u128,
        calls / iterations,
        bytes / iterations
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let case = args
        .next()
        .expect("case: loop | planning | sqlite | memory-search");
    let iterations: usize = args.next().map_or(10, |value| value.parse().unwrap());
    assert!(iterations > 0 && args.next().is_none());
    let directory = Directory::new();
    match case.as_str() {
        "loop" | "planning" => measure(&case, iterations, |id| {
            run(MemoryRunStore::new(), &directory, id, case == "planning");
        }),
        #[cfg(feature = "sqlite")]
        "sqlite" => measure(&case, iterations, |id| {
            run(
                agent_core::agent::runtime::SqliteRunStore::open(directory.0.join("runs.sqlite3"))
                    .unwrap(),
                &directory,
                id,
                false,
            );
        }),
        "memory-search" => {
            let mut store = MarkdownMemoryStore::open(directory.0.join("memories")).unwrap();
            for id in 0..512 {
                let memory = Memory::new(format!("note-{id:04}"), "local evidence ".repeat(2048));
                let memory = if id % 64 == 0 {
                    memory.with_tag("needle")
                } else {
                    memory
                };
                store.save(memory).unwrap();
            }
            measure(&case, iterations, |_| {
                let matches = store.search("needle").unwrap();
                assert_eq!(matches.len(), 8);
                black_box(matches);
            });
        }
        _ => panic!("unknown benchmark case: {case}"),
    }
}
