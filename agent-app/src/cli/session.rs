use super::commands::Command;
use agent_core::{
    agent::runtime::{
        RunLimits, RunOptions, RunState, RunStatus, Runtime, SqliteRunStore, WorkIntent,
    },
    memory::MarkdownMemoryStore,
    model::ModelProvider,
    tool::{Registry, ToolOutput},
    trace::FileTraceSink,
};
use std::{
    env,
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
}

impl<M: ModelProvider> Session<M> {
    pub(super) fn new(model: M) -> Result<Self, Box<dyn Error>> {
        let store = SqliteRunStore::open(super::db_path())?;
        let memory = MarkdownMemoryStore::open(
            env::var("RS_AGENT_MEMORY_DIR").unwrap_or_else(|_| "memories".into()),
        )?;
        let mut tools = Registry::new();
        tools.register(crate::tools::echo_tool())?;
        tools.register(crate::tools::session_finish_tool())?;
        tools.register(crate::tools::WriteFile::new())?;
        tools.register(crate::tools::RunCmd::new())?;
        tools.register(crate::tools::ReadFile::new())?;
        tools.register(crate::tools::ListDirectory::new())?;
        tools.register(crate::tools::SearchFiles::new())?;
        let max_steps =
            env::var("RS_AGENT_MAX_STEPS").map_or(Ok(8), |value| value.parse::<u64>())?;
        if max_steps == 0 {
            return Err("RS_AGENT_MAX_STEPS 必须大于零".into());
        }
        let runtime = Runtime::new(model, store, memory, tools)
            .with_trace_sink(FileTraceSink::open(super::trace_path())?);
        Ok(Self {
            runtime,
            session_id: env::var("RS_AGENT_SESSION").unwrap_or_else(|_| "default".into()),
            limits: RunLimits::new(max_steps),
        })
    }

    pub(super) fn handle(&mut self, command: Command<'_>) -> Result<bool, Box<dyn Error>> {
        match command {
            Command::Exit => return Ok(true),
            Command::Help => super::help(),
            Command::Trace => super::show_trace()?,
            Command::Reset => {
                self.runtime.store_mut().reset_session(&self.session_id)?;
                println!("当前会话历史已清除。");
            }
            Command::Input(input) => {
                let state = self.start(input, WorkIntent::Execute)?;
                return self.execute(state.id(), false);
            }
            Command::Start(input) => {
                let state = self.start(input, WorkIntent::Execute)?;
                super::print_status(&state);
            }
            Command::Status(id) => {
                let id = self.id(id)?;
                super::print_status(&self.runtime.state(&id)?);
            }
            Command::Plan(Some(input)) => {
                let state = self.start(input, WorkIntent::PlanOnly)?;
                return self.execute(state.id(), false);
            }
            Command::Plan(None) => {
                let id = self.id(None)?;
                super::print_plan(&self.runtime.state(&id)?);
            }
            Command::Execute(id) => {
                let id = self.id(id)?;
                self.runtime.execute_plan(&id)?;
                return self.execute(&id, false);
            }
            Command::Board(key) => {
                let id = self.id(None)?;
                let state = self.runtime.state(&id)?;
                let entries = match key {
                    Some(key) => state.blackboard().latest(key).into_iter().collect(),
                    None => state.blackboard().changes(0, 32),
                };
                println!("{}", serde_json::to_string_pretty(&entries)?);
            }
            Command::Resume(id) => {
                let id = self.id(id)?;
                return self.execute(&id, false);
            }
            Command::Step(id) => {
                let id = self.id(id)?;
                return self.execute(&id, true);
            }
            Command::Pause(id) => {
                let id = self.id(id)?;
                super::print_status(&self.runtime.pause(&id)?);
            }
            Command::Cancel(id) => {
                let id = self.id(id)?;
                super::print_status(&self.runtime.cancel(&id)?);
            }
            Command::Budget(max_steps, id) => {
                let id = self.id(id)?;
                super::print_status(&self.runtime.set_max_steps(&id, max_steps)?);
            }
            Command::Resolve(call_id, output) => {
                let id = self.id(None)?;
                super::print_status(&self.runtime.resolve_tool(
                    &id,
                    call_id,
                    ToolOutput::text(output),
                )?);
            }
            Command::Retry(call_id) => {
                let id = self.id(None)?;
                super::print_status(&self.runtime.retry_tool(&id, call_id)?);
            }
        }
        Ok(false)
    }

    fn id(&self, id: Option<&str>) -> Result<String, Box<dyn Error>> {
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

    fn start(&mut self, input: &str, intent: WorkIntent) -> Result<RunState, Box<dyn Error>> {
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

    fn execute(&mut self, id: &str, single_step: bool) -> Result<bool, Box<dyn Error>> {
        println!();
        io::stdout().flush()?;
        let mut output = crate::output::StreamingOutput::new(io::stdout().lock());
        let mut output_error = None;
        let mut emit = |delta: &str| {
            if output_error.is_none() {
                output_error = output.push(delta).err();
            }
        };
        let result = if single_step {
            self.runtime.advance(id, &mut emit)
        } else {
            self.runtime.resume(id, &mut emit)
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
                    super::print_status(&state);
                }
                return Err(error.into());
            }
        };
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
            super::print_status(&state);
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
