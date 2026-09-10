//! Read a bounded dotenv file without modifying process-global environment state.
use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{self, Read},
    path::Path,
};

const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_VARIABLES: usize = 256;

#[derive(Default)]
pub(crate) struct Environment {
    file_values: BTreeMap<String, String>,
}

impl Environment {
    pub(crate) fn load() -> io::Result<Self> {
        match env::var_os("RS_AGENT_ENV_FILE") {
            Some(path) if path.is_empty() => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RS_AGENT_ENV_FILE 不能为空",
            )),
            Some(path) => Self::from_path(Path::new(&path), true),
            None => Self::from_path(Path::new(".env"), false),
        }
    }

    fn from_path(path: &Path, required: bool) -> io::Result<Self> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if !required && error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("无法读取环境文件 {}：{error}", path.display()),
                ));
            }
        };
        if !metadata.is_file() {
            return Err(invalid(path, "必须是普通文件"));
        }
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(invalid(path, "超过 64 KiB 上限"));
        }
        let file = File::open(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("无法读取环境文件 {}：{error}", path.display()),
            )
        })?;
        let mut source = String::new();
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_string(&mut source)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("环境文件 {} 读取失败或不是 UTF-8 文本", path.display()),
                )
            })?;
        if source.len() > MAX_FILE_BYTES {
            return Err(invalid(path, "超过 64 KiB 上限"));
        }
        Self::parse(source.strip_prefix('\u{feff}').unwrap_or(&source), path)
    }

    fn parse(source: &str, path: &Path) -> io::Result<Self> {
        let mut file_values = BTreeMap::new();
        let mut bytes = 0usize;
        for item in dotenvy::from_read_iter(source.as_bytes()) {
            let (name, value) = item.map_err(|error| {
                // dotenvy errors can contain the full input line, including secrets.
                // Report only its location, never the original diagnostic or value.
                let line = match error {
                    dotenvy::Error::LineParse(line, _) => {
                        source.find(line.trim_end()).map(|offset| {
                            source[..offset]
                                .bytes()
                                .filter(|byte| *byte == b'\n')
                                .count()
                                + 1
                        })
                    }
                    _ => None,
                };
                invalid(
                    path,
                    &line.map_or_else(
                        || "格式错误，请检查赋值语法和引号".into(),
                        |line| format!("第 {line} 行附近格式错误，请检查赋值语法和引号"),
                    ),
                )
            })?;
            bytes = bytes.saturating_add(name.len()).saturating_add(value.len());
            if file_values.len() >= MAX_VARIABLES || bytes > MAX_FILE_BYTES {
                return Err(invalid(path, "变量数超过 256 或解析后的内容超过 64 KiB"));
            }
            if name.contains('\0') || value.contains('\0') {
                return Err(invalid(path, "变量不能包含 NUL 字符"));
            }
            if file_values.insert(name, value).is_some() {
                return Err(invalid(path, "存在重复变量定义"));
            }
        }
        Ok(Self { file_values })
    }

    pub(crate) fn var(&self, name: &str) -> Result<String, env::VarError> {
        self.resolve(name, env::var(name))
    }

    fn resolve(
        &self,
        name: &str,
        process_value: Result<String, env::VarError>,
    ) -> Result<String, env::VarError> {
        match process_value {
            Err(env::VarError::NotPresent) => self
                .file_values
                .get(name)
                .cloned()
                .ok_or(env::VarError::NotPresent),
            other => other,
        }
    }
}

fn invalid(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("环境文件 {}：{reason}", path.display()),
    )
}

#[cfg(test)]
mod tests;
