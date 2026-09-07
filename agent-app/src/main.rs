use std::{
    env,
    error::Error,
    io::{self, BufRead, Write},
    process,
};

use crate::provider::{AnthropicProvider, OpenAiCompatibleProvider};
use agent_core::{
    agent::Agent,
    context::{Context, ContextStore, SqliteContextStore},
    memory::MarkdownMemoryStore,
    model::ModelProvider,
    tool,
    tool::Registry,
    trace::{FileTraceSink, TraceSink},
};

mod provider;

#[tool]
fn echo(text: String) -> String {
    text
}

#[tool(
    finish_session,
    description = "当用户明确表示要结束、退出、退下或今天到此为止时调用。summary 必须简洁概括本次会话的重要结论。"
)]
fn session_finish(summary: String) -> String {
    summary
}

fn main() {
    if let Err(error) = run() {
        eprintln!("启动失败：{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let provider = env::var("RS_AGENT_PROVIDER").unwrap_or_else(|_| "openai".to_owned());

    match provider.as_str() {
        "openai" | "openai-compatible" => {
            let api_key = required_environment("OPENAI_API_KEY")?;
            let model = required_environment("OPENAI_MODEL")?;
            let mut provider = OpenAiCompatibleProvider::new(api_key, model);
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
    tools.register(echo_tool())?;
    tools.register(session_finish_tool())?;
    let trace_path =
        env::var("RS_AGENT_TRACE_FILE").unwrap_or_else(|_| "agent-trace.jsonl".to_owned());
    let trace_sink = FileTraceSink::open(trace_path)?;
    let mut agent =
        Agent::new(model, context_store, memory_store, tools).with_trace_sink(trace_sink);
    let session_id = env::var("RS_AGENT_SESSION").unwrap_or_else(|_| "default".to_owned());

    println!("rs-agent 已启动。输入 /help 查看命令。");
    let stdin = io::stdin();
    let mut input = stdin.lock();

    loop {
        print!("\n你 > ");
        io::stdout().flush()?;

        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            println!();
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "/exit" | "/quit" => break,
            "/help" => print_help(),
            "/reset" => reset_session(&mut agent, &session_id)?,
            message => {
                print!("\n助手 > ");
                io::stdout().flush()?;
                let mut print_delta = |delta: &str| {
                    print!("{delta}");
                    let _ = io::stdout().flush();
                };
                match agent.run_stream(&session_id, message, &mut print_delta) {
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
    println!("命令：\n  /help  显示帮助\n  /reset 清除当前会话历史\n  /exit  退出程序");
}
