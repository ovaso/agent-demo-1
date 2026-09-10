use crate::{terminal, trace_map};
use agent_core::{
    agent::runtime::{RunState, RunStatus, SqliteRunStore},
    model::ModelProvider,
};
use rustyline::error::ReadlineError;
use std::{env, error::Error, io};

mod commands;
mod session;
#[cfg(test)]
mod tests;

pub(crate) fn run<M: ModelProvider>(model: M) -> Result<(), Box<dyn Error>> {
    let mut session = session::Session::new(model)?;
    println!("rs-agent 已启动。Enter 发送，Ctrl+J / Alt+Enter 换行。输入 /help 查看命令。");
    if let Some(state) = session.runtime.store().latest(&session.session_id)?
        && !matches!(state.status(), RunStatus::Completed | RunStatus::Cancelled)
    {
        println!("发现未结束任务，可用 /resume 继续或 /status 查看。");
        print_status(&state);
    }
    let mut editor = terminal::editor()?;
    loop {
        let line = match editor.readline(terminal::USER_PROMPT) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if line.trim().is_empty() {
            continue;
        }
        editor.add_history_entry(line.as_str())?;
        let command = match commands::parse(&line) {
            Ok(command) => command,
            Err(error) => {
                eprintln!("{error}");
                continue;
            }
        };
        match session.handle(command) {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => eprintln!("请求失败：{error}"),
        }
    }
    Ok(())
}

pub(crate) fn trace_path() -> String {
    env::var("RS_AGENT_TRACE_FILE").unwrap_or_else(|_| "agent-trace.jsonl".into())
}

fn db_path() -> String {
    env::var("RS_AGENT_DB").unwrap_or_else(|_| "agent-context.sqlite3".into())
}

pub(crate) fn show_status(id: Option<&str>) -> Result<(), Box<dyn Error>> {
    use agent_core::agent::runtime::RunStore;
    let store = SqliteRunStore::open(db_path())?;
    let state = match id {
        Some(id) => store.load(id)?,
        None => store.latest(&env::var("RS_AGENT_SESSION").unwrap_or_else(|_| "default".into()))?,
    };
    match state {
        Some(state) => print_status(&state),
        None => println!("没有匹配的运行记录。"),
    }
    Ok(())
}

fn print_status(state: &RunState) {
    println!(
        "执行方式：{:?}；待切换：{:?}；活动节点：{:?}",
        state.routing().mode(),
        state.routing().pending_mode(),
        state.graph().active_node()
    );
    println!(
        "运行：{}\n状态：{:?}\n阶段：{:?}\n模型步数：{}/{}；工具调用：{}/{}；检查点：{}",
        state.id(),
        state.status(),
        state.phase(),
        state.budget().model_calls(),
        state.limits().max_steps,
        state.budget().tool_calls(),
        state.limits().max_tool_calls,
        state.revision()
    );
    if let Some(result) = state.result() {
        println!("结果：{}", result.text());
    }
    if state.status() == &RunStatus::Paused(agent_core::agent::runtime::PauseReason::PlanReady) {
        print_plan(state);
        println!("计划已保存；/execute 开始执行，/resume 保持只规划边界。");
    }
}

fn print_graph(state: &RunState) {
    if let Some(graph) = state.graph().current() {
        println!("图计划 v{}", graph.plan_version);
        for (id, node) in &graph.nodes {
            println!(
                "  [{id}] {:?}，尝试 {}，验收依据 {:?}\n    {}",
                node.status, node.attempts, node.validation, node.task.description
            );
        }
    } else {
        println!("尚未启动图执行。");
    }
}

fn print_plan(state: &RunState) {
    match state.plans().current() {
        Some(plan) => {
            println!("计划 v{}：{}", state.plans().revision(), plan.goal);
            println!("总体要求：{}", plan.requirements.join("；"));
            for task in &plan.tasks {
                println!(
                    "  [{}] {}\n    依赖：{}；验收：{}",
                    task.id,
                    task.description,
                    task.depends_on.join(", "),
                    task.acceptance.join("；")
                );
            }
        }
        None => println!("尚未提交结构化计划。"),
    }
}

fn help() {
    println!(
        "命令：
  /start <任务>          建立任务，随后可用 /step 逐步执行
  /plan <任务>          只调查和规划，不执行写入
  /plan                 查看当前计划
  /execute [运行 ID]    执行已交付的计划，沿用原预算
  /board [记录标识]     查看共享记录（未指定时最多 32 条）
  /mode [loop|graph]    查看或选择执行方式
  /graph               查看图节点状态
  /retry-node <节点ID>  明确重试已知失败节点，随后 /resume
  /status [运行 ID]     查看状态、预算和最终结果
  /resume [运行 ID]     恢复执行，沿用原预算
  /step [运行 ID]       推进一个阶段（暂停后需 /resume）
  /pause [运行 ID]      暂停待执行任务
  /cancel [运行 ID]     取消任务，保留记录
  /budget <总步数> [ID] 明确修改模型总额度
  /resolve <调用 ID> <结果>  提交已核实的工具结果
  /retry <调用 ID>      明确重试结果未知的工具
  /trace               查看调用树
  /reset               清除已结束任务的会话历史
  /exit                退出（未结束任务可恢复）
普通文本直接开始并执行任务；控制命令省略 ID 时使用当前会话最新任务。
Enter 发送，Ctrl+J / Alt+Enter 换行；Ctrl+C 取消输入，Ctrl+D 退出。"
    );
}

fn show_trace() -> Result<(), Box<dyn Error>> {
    trace_map::show(trace_path(), &mut io::stdout().lock())?;
    Ok(())
}
