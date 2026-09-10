pub mod http;

use agent_core::agent::runtime::{RunState, SqliteRunStore};
use http::{Provider, Step};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{ErrorKind, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub directory: PathBuf,
}

pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub requests: Vec<Value>,
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Fixture {
    pub fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rs-agent-cli-e2e-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self { directory }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-app"));
        // The subprocess cannot inherit real API keys, proxies, or the user's database paths.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("TERM", "dumb")
            .env("RS_AGENT_DB", self.directory.join("runs.sqlite3"))
            .env("RS_AGENT_MEMORY_DIR", self.directory.join("memories"))
            .env("RS_AGENT_TRACE_FILE", self.directory.join("trace.jsonl"))
            .env("RS_AGENT_SESSION", "e2e")
            .env("RS_AGENT_MAX_STEPS", "3")
            .current_dir(&self.directory);
        command
    }

    pub fn offline(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    pub fn state(&self) -> RunState {
        SqliteRunStore::open(self.directory.join("runs.sqlite3"))
            .unwrap()
            .latest("e2e")
            .unwrap()
            .unwrap()
    }

    pub fn run(&self, provider: Provider, input: &str, steps: &[Step]) -> CliResult {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let mut command = self.command();
        match provider {
            Provider::OpenAi => {
                command
                    .env("RS_AGENT_PROVIDER", "openai")
                    .env("OPENAI_BASE_URL", url)
                    .env("OPENAI_API_KEY", "test-key")
                    .env("OPENAI_MODEL", "test-model");
            }
            Provider::Anthropic => {
                command
                    .env("RS_AGENT_PROVIDER", "anthropic")
                    .env("ANTHROPIC_BASE_URL", url)
                    .env("ANTHROPIC_API_KEY", "test-key")
                    .env("ANTHROPIC_MODEL", "test-model");
            }
        }
        let stdout_path = self.directory.join("stdout.log");
        let stderr_path = self.directory.join("stderr.log");
        let mut process = Process(
            command
                .stdin(Stdio::piped())
                .stdout(File::create(&stdout_path).unwrap())
                .stderr(File::create(&stderr_path).unwrap())
                .spawn()
                .unwrap(),
        );
        process
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut requests = Vec::new();
        let status = loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let step = steps.get(requests.len()).unwrap_or_else(|| {
                        panic!("unexpected model request {}", requests.len() + 1)
                    });
                    requests.push(http::exchange(stream, provider, step));
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => panic!("accept: {error}"),
            }
            if let Some(status) = process.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "CLI timeout: {}\n{}",
                fs::read_to_string(&stdout_path).unwrap(),
                fs::read_to_string(&stderr_path).unwrap()
            );
            thread::sleep(Duration::from_millis(5));
        };
        let stdout = fs::read_to_string(stdout_path).unwrap();
        let stderr = fs::read_to_string(stderr_path).unwrap();
        assert!(status.success(), "CLI failed: {stdout}\n{stderr}");
        if steps.iter().all(|step| step.complete) {
            assert!(stderr.is_empty(), "CLI reported an error: {stderr}");
        }
        assert_eq!(requests.len(), steps.len(), "missing request: {stdout}");
        CliResult {
            stdout,
            stderr,
            requests,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
