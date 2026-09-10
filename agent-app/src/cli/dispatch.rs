use super::{commands::Command, session::Session, view};
use agent_core::{agent::runtime::WorkIntent, model::ModelProvider, tool::ToolOutput};
use std::error::Error;

impl<M: ModelProvider> Session<M> {
    pub(super) fn handle(&mut self, command: Command<'_>) -> Result<bool, Box<dyn Error>> {
        match command {
            Command::Exit => return Ok(true),
            Command::Help => view::help(),
            Command::Trace => view::show_trace(&self.trace_path)?,
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
                view::print_status(&state);
            }
            Command::Status(id) => {
                let id = self.id(id)?;
                view::print_status(&self.runtime.state(&id)?);
            }
            Command::Plan(Some(input)) => {
                let state = self.start(input, WorkIntent::PlanOnly)?;
                return self.execute(state.id(), false);
            }
            Command::Plan(None) => {
                let id = self.id(None)?;
                view::print_plan(&self.runtime.state(&id)?);
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
            Command::Mode(mode) => {
                let id = self.id(None)?;
                let state = match mode {
                    Some(mode) => self.runtime.route(&id, mode, "用户选择执行方式")?,
                    None => self.runtime.state(&id)?,
                };
                view::print_status(&state);
            }
            Command::Graph => {
                let id = self.id(None)?;
                view::print_graph(&self.runtime.state(&id)?);
            }
            Command::Agents => {
                let id = self.id(None)?;
                view::print_agents(&self.runtime.state(&id)?);
            }
            Command::Messages(message) => {
                let id = self.id(None)?;
                view::print_messages(&self.runtime.state(&id)?, message)?;
            }
            Command::Message(to, body) => {
                let id = self.id(None)?;
                self.runtime.send_notice(&id, to, body)?;
                println!("通知已保存，暂停状态保持不变。");
            }
            Command::Reply(request, body) => {
                let id = self.id(None)?;
                self.runtime.answer_request(&id, request, body)?;
                println!("答复已保存，使用 /resume 继续。");
            }
            Command::AgentBudget(agent, max_steps) => {
                let id = self.id(None)?;
                view::print_agents(&self.runtime.set_agent_budget(&id, agent, max_steps)?);
            }
            Command::CancelAgent(agent) => {
                let id = self.id(None)?;
                view::print_agents(&self.runtime.cancel_agent(&id, agent)?);
            }
            Command::RetryNode(node) => {
                let id = self.id(None)?;
                view::print_status(&self.runtime.retry_node(&id, node)?);
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
                view::print_status(&self.runtime.pause(&id)?);
            }
            Command::Cancel(id) => {
                let id = self.id(id)?;
                view::print_status(&self.runtime.cancel(&id)?);
            }
            Command::Tokens(limit) => {
                let id = self.id(None)?;
                let state = if let Some(limit) = limit {
                    self.runtime
                        .set_token_budget(&id, (limit > 0).then_some(limit))?
                } else {
                    self.runtime.state(&id)?
                };
                view::print_status(&state);
            }
            Command::OutputBudget(limit) => {
                let id = self.id(None)?;
                view::print_status(&self.runtime.set_output_budget(&id, limit)?);
            }
            Command::Budget(max_steps, id) => {
                let id = self.id(id)?;
                view::print_status(&self.runtime.set_max_steps(&id, max_steps)?);
            }
            Command::BudgetAuto(hard_max_steps, id) => {
                let id = self.id(id)?;
                let policy = agent_core::agent::runtime::StepExtensionPolicy {
                    hard_max_steps,
                    ..self.limits.step_extension.clone().unwrap_or_default()
                };
                view::print_status(&self.runtime.set_step_extension_policy(&id, policy)?);
            }
            Command::Resolve(call_id, output) => {
                let id = self.id(None)?;
                view::print_status(&self.runtime.resolve_tool(
                    &id,
                    call_id,
                    ToolOutput::text(output),
                )?);
            }
            Command::Retry(call_id) => {
                let id = self.id(None)?;
                view::print_status(&self.runtime.retry_tool(&id, call_id)?);
            }
        }
        Ok(false)
    }
}
