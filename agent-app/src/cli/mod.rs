use crate::{config, terminal};
use agent_core::{
    agent::runtime::{RunStatus, SqliteRunStore},
    model::ModelProvider,
};
use rustyline::error::ReadlineError;
use std::error::Error;

mod commands;
mod session;
#[cfg(test)]
mod tests;
mod view;

pub(crate) fn run<M: ModelProvider>(
    model: M,
    config: config::RuntimeConfig,
    debug_snapshot: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let mut session = session::Session::new(model, config, debug_snapshot)?;
    println!("rs-agent 已启动。Enter 发送，Ctrl+J / Alt+Enter 换行。输入 /help 查看命令。");
    if let Some(state) = session.runtime.store().latest(&session.session_id)?
        && !matches!(state.status(), RunStatus::Completed | RunStatus::Cancelled)
    {
        println!("发现未结束任务，可用 /resume 继续或 /status 查看。");
        view::print_status(&state);
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

pub(crate) fn show_status(
    id: Option<&str>,
    environment: &config::Environment,
) -> Result<(), Box<dyn Error>> {
    use agent_core::agent::runtime::RunStore;
    let store = SqliteRunStore::open(config::db_path(environment))?;
    let state = match id {
        Some(id) => store.load(id)?,
        None => store.latest(&config::session_id(environment))?,
    };
    match state {
        Some(state) => view::print_status(&state),
        None => println!("没有匹配的运行记录。"),
    }
    Ok(())
}
