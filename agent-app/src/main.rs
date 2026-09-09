use std::{
    env,
    error::Error,
    io::{self, Write},
    process,
};

use rustyline::error::ReadlineError;

use crate::provider::{AnthropicProvider, OpenAiCompatibleProvider};
use agent_core::{
    agent::Agent,
    context::{Context, ContextStore, SqliteContextStore},
    memory::MarkdownMemoryStore,
    model::ModelProvider,
    tool::Registry,
    trace::{FileTraceSink, TraceSink},
};

mod output;
mod provider;
mod terminal;
mod tools;
mod trace_map;

fn main() {
    if let Err(error) = run() {
        eprintln!("启动失败：{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--trace-map")) {
        let path = args
            .next()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| trace_path().into());
        if args.next().is_some() {
            return Err("用法：agent-app --trace-map [JSONL 文件]".into());
        }
        trace_map::show(path, &mut io::stdout().lock())?;
        return Ok(());
    }
    let provider = env::var("RS_AGENT_PROVIDER").unwrap_or_else(|_| "openai".to_owned());

    match provider.as_str() {
        "openai" | "openai-compatible" => {
            let api_key = required_environment("OPENAI_API_KEY")?;
            let model = required_environment("OPENAI_MODEL")?;
            let stream_usage = match env::var("OPENAI_STREAM_USAGE").as_deref() {
                Ok("0" | "false") => false,
                Ok("1" | "true") | Err(env::VarError::NotPresent) => true,
                _ => return Err("OPENAI_STREAM_USAGE 必须为 true/false 或 1/0".into()),
            };
            let mut provider =
                OpenAiCompatibleProvider::new(api_key, model).with_stream_usage(stream_usage);
            if let Ok(base_url) = env::var("OPENAI_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            run_cli(provider)
        }
        "anthropic" => {
            let api_key = required_environment("ANTHROPIC_API_KEY")?;
            let model = required_environment("ANTHROPIC_MODEL")?;
            let max_tokens = env::var("ANTHROPIC_MAX_TOKENS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1_024);
            let mut provider = AnthropicProvider::new(api_key, model).with_max_tokens(max_tokens);
            if let Ok(base_url) = env::var("ANTHROPIC_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            run_cli(provider)
        }
        other => Err(format!(
            "不支持的 RS_AGENT_PROVIDER：{other}；可选值为 openai、openai-compatible 或 anthropic"
        )
        .into()),
    }
}

fn required_environment(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("缺少环境变量 {name}").into())
}

fn run_cli<M>(model: M) -> Result<(), Box<dyn Error>>
where
    M: ModelProvider,
{
    let context_store = SqliteContextStore::open("agent-context.sqlite3")?;
    let memory_store = MarkdownMemoryStore::open("memories")?;
    let mut tools = Registry::new();
    tools.register(tools::echo_tool())?;
    tools.register(tools::session_finish_tool())?;
    tools.register(tools::WriteFile::new())?;
    tools.register(tools::RunCmd::new())?;
    let trace_path = trace_path();
    let trace_sink = FileTraceSink::open(&trace_path)?;
    let mut agent =
        Agent::new(model, context_store, memory_store, tools).with_trace_sink(trace_sink);
    let session_id = env::var("RS_AGENT_SESSION").unwrap_or_else(|_| "default".to_owned());

    println!("rs-agent 已启动。Enter 发送，Ctrl+J / Alt+Enter 换行。输入 /help 查看命令。");
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
            Err(error) => return Err(Box::new(error)),
        };
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        editor.add_history_entry(line.as_str())?;

        match command {
            "/exit" | "/quit" => break,
            "/help" => print_help(),
            "/trace" => {
                if let Err(error) = trace_map::show(&trace_path, &mut io::stdout().lock()) {
                    eprintln!("调用链读取失败：{error}");
                }
            }
            "/reset" => reset_session(&mut agent, &session_id)?,
            _ => {
                println!();
                io::stdout().flush()?;
                let mut output = output::StreamingOutput::new(io::stdout().lock());
                let mut output_error = None;
                let result = agent.run_stream(&session_id, &line, &mut |delta| {
                    if output_error.is_none() {
                        output_error = output.push(delta).err();
                    }
                });
                if let Some(error) = output_error {
                    return Err(error.into());
                }
                output.finish()?;
                match result {
                    Ok(result) if result.session_finished() => {
                        println!("\n会话已结束：{}", result.text());
                        break;
                    }
                    Ok(_) => println!(),
                    Err(error) => eprintln!("\n请求失败：{error}"),
                }
            }
        }
    }

    Ok(())
}

fn reset_session<M, T>(
    agent: &mut Agent<M, SqliteContextStore, MarkdownMemoryStore, T>,
    session_id: &str,
) -> Result<(), Box<dyn Error>>
where
    T: TraceSink,
{
    let mut context = agent
        .context_store()
        .load(session_id)?
        .unwrap_or_else(Context::new);
    context.clear_history();
    agent.context_store_mut().save(session_id, &context)?;
    println!("当前会话历史已清除。");
    Ok(())
}

fn print_help() {
    println!(
        "命令：\n  /help  显示帮助\n  /trace 查看调用树\n  /reset 清除当前会话历史\n  /exit  退出程序"
    );
    println!(
        "输入：\n  Enter            发送\n  Ctrl+J / Alt+Enter 换行\n  Ctrl+C           取消当前输入\n  Ctrl+D           空输入时退出"
    );
}

fn trace_path() -> String {
    env::var("RS_AGENT_TRACE_FILE").unwrap_or_else(|_| "agent-trace.jsonl".to_owned())
}
