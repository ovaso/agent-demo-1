//! Agent 执行链路的结构化文件追踪。

mod run;
pub(crate) use run::RunTrace;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;

/// 一条可序列化为 JSON Lines 的追踪事件。
#[derive(Debug, Serialize)]
pub struct TraceEvent {
    timestamp_ms: u128,
    name: String,
    fields: BTreeMap<String, Value>,
}

impl TraceEvent {
    pub fn new(name: impl Into<String>) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            timestamp_ms,
            name: name.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn with_field(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.insert(name.into(), value.into());
        self
    }
}

/// 追踪事件的输出目标。
pub trait TraceSink {
    fn record(&mut self, event: TraceEvent) -> Result<(), TraceError>;
}

/// 默认追踪目标：丢弃所有事件。
#[derive(Debug, Default)]
pub struct NoopTraceSink;

impl TraceSink for NoopTraceSink {
    fn record(&mut self, _event: TraceEvent) -> Result<(), TraceError> {
        Ok(())
    }
}

/// 将每条事件作为一行 JSON 追加到文件。
pub struct FileTraceSink {
    output: BufWriter<File>,
}

impl FileTraceSink {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TraceError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(TraceError::io)?;
        }
        let output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(TraceError::io)?;
        Ok(Self {
            output: BufWriter::new(output),
        })
    }
}

impl TraceSink for FileTraceSink {
    fn record(&mut self, event: TraceEvent) -> Result<(), TraceError> {
        serde_json::to_writer(&mut self.output, &event).map_err(TraceError::serialization)?;
        self.output.write_all(b"\n").map_err(TraceError::io)?;
        self.output.flush().map_err(TraceError::io)
    }
}

#[derive(Debug)]
pub enum TraceError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
}

impl TraceError {
    fn io(error: std::io::Error) -> Self {
        Self::Io(error)
    }

    fn serialization(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl Display for TraceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "追踪文件写入失败：{error}"),
            Self::Serialization(error) => write!(formatter, "追踪事件序列化失败：{error}"),
        }
    }
}

impl Error for TraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
        }
    }
}
