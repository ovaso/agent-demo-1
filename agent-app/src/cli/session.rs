use super::view;
use crate::config::RuntimeConfig;
use agent_core::{
    agent::runtime::{
        RunLimits, RunOptions, RunState, RunStatus, Runtime, SqliteRunStore, WorkIntent,
    },
    memory::MarkdownMemoryStore,
    model::{ModelProvider, ModelStreamEvent},
    tool::Registry,
    trace::FileTraceSink,
};
use std::{
    error::Error,
    io::{self, Write},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

type AppRuntime<M> = Runtime<M, SqliteRunStore, MarkdownMemoryStore, FileTraceSink>;

pub(super) struct Session<M> {
    pub(super) runtime: AppRuntime<M>,
    pub(super) session_id: String,
    pub(super) limits: RunLimits,
    pub(super) trace_path: String,
}

impl<M: ModelProvider> Session<M> {
    pub(super) fn new(
        model: M,
        config: RuntimeConfig,
        debug_snapshot: Option<String>,
    ) -> Result<Self, Box<dyn Error>> {
        let store = SqliteRunStore::open(config.db_path)?;
        let memory = MarkdownMemoryStore::open(config.memory_directory)?;
        let mut tools = Registry::new();
        agent_app_tool::register(&mut tools)?;
        if let Some(snapshot) = debug_snapshot {
            agent_tool_debug::register(&mut tools, snapshot)?;
        }
        let runtime = Runtime::new(model, store, memory, tools)
            .with_trace_sink(FileTraceSink::open(&config.trace_path)?);
        Ok(Self {
            runtime,
            session_id: config.session_id,
            limits: config.limits,
            trace_path: config.trace_path,
        })
    }

    pub(super) fn id(&self, id: Option<&str>) -> Result<String, Box<dyn Error>> {
        match id {
            Some(id) => Ok(id.to_owned()),
            None => self
                .runtime
                .store()
                .latest(&self.session_id)?
                .map(|state| state.id().to_owned())
                .ok_or_else(|| "当前会话还没有运行任务".into()),
        }
    }

    pub(super) fn start(
        &mut self,
        input: &str,
        intent: WorkIntent,
    ) -> Result<RunState, Box<dyn Error>> {
        if self
            .runtime
            .store()
            .latest(&self.session_id)?
            .is_some_and(|state| {
                !matches!(state.status(), RunStatus::Completed | RunStatus::Cancelled)
            })
        {
            return Err("当前会话有未结束任务，请用 /resume 继续或 /cancel 取消".into());
        }
        let context = self
            .runtime
            .store()
            .session_context(&self.session_id)?
            .unwrap_or_default();
        let id = new_id();
        let state = self.runtime.start_with_options(
            &id,
            &self.session_id,
            input,
            context,
            RunOptions {
                limits: self.limits.clone(),
                intent,
                planning: true,
            },
        )?;
        println!("运行：{id}");
        Ok(state)
    }

    pub(super) fn execute(&mut self, id: &str, single_step: bool) -> Result<bool, Box<dyn Error>> {
        println!();
        io::stdout().flush()?;
        let mut output = crate::output::StreamingOutput::new(io::stdout().lock());
        let mut output_error = None;
        let mut emit = |event: ModelStreamEvent<'_>| {
            if output_error.is_none() {
                output_error = output.push_event(event).err();
            }
        };
        let result = if single_step {
            self.runtime.advance_events(id, &mut emit)
        } else {
            self.runtime.resume_events(id, &mut emit)
        };
        if let Some(error) = output_error {
            return Err(error.into());
        }
        output.finish()?;
        println!();
        let state = match result {
            Ok(state) => state,
            Err(error) => {
                if let Ok(state) = self.runtime.state(id) {
                    view::print_status(&state);
                }
                return Err(error.into());
            }
        };
        if state.status() == &RunStatus::Completed && !state.budget().step_extensions().is_empty() {
            view::print_budget(&state);
        }
        if state
            .result()
            .is_some_and(|result| result.session_finished())
        {
            println!(
                "会话已结束：{}",
                state.result().expect("finished result").text()
            );
            return Ok(true);
        }
        if single_step || state.status() != &RunStatus::Completed {
            view::print_status(&state);
        }
        Ok(false)
    }
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
