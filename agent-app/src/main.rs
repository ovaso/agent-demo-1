use std::{env, error::Error, io, process};

mod cli;
mod config;
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
            .unwrap_or_else(|| config::trace_path().into());
        if args.next().is_some() {
            return Err("用法：agent-app --trace-map [JSONL 文件]".into());
        }
        trace_map::show(path, &mut io::stdout().lock())?;
        return Ok(());
    }
    if command.is_some() {
        return Err("支持的参数：--status [运行 ID]、--trace-map [文件]".into());
    }
    cli::run(config::model_provider()?)
}
