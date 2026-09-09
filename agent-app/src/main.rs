use crate::provider::{AnthropicProvider, OpenAiCompatibleProvider};
use std::{env, error::Error, io, process};

mod cli;
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
    let command = args.next();
    if command.as_deref() == Some(std::ffi::OsStr::new("--status")) {
        let id = args
            .next()
            .map(|value| value.to_string_lossy().into_owned());
        if args.next().is_some() {
            return Err("用法：agent-app --status [运行 ID]".into());
        }
        return cli::show_status(id.as_deref());
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("--trace-map")) {
        let path = args
            .next()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| cli::trace_path().into());
        if args.next().is_some() {
            return Err("用法：agent-app --trace-map [JSONL 文件]".into());
        }
        trace_map::show(path, &mut io::stdout().lock())?;
        return Ok(());
    }
    if command.is_some() {
        return Err("支持的参数：--status [运行 ID]、--trace-map [文件]".into());
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
            cli::run(provider)
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
            cli::run(provider)
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
