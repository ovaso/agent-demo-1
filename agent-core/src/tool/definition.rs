use serde::{Deserialize, Serialize};

/// 描述工具接受的一个输入参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    name: String,
    description: String,
    required: bool,
}

impl Parameter {
    pub fn required(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn is_required(&self) -> bool {
        self.required
    }
}

/// LLM 选择和调用工具所需的元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(default)]
    created_at: u64,
    name: String,
    #[serde(default)]
    version: String,
    description: String,
    parameters: Vec<Parameter>,
    #[serde(default)]
    read_only: bool,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Vec<Parameter>,
    ) -> Self {
        Self {
            created_at: 0,
            name: name.into(),
            version: String::new(),
            description: description.into(),
            parameters,
            read_only: false,
        }
    }

    /// Attach source-controlled metadata. Existing timestamps must not be changed
    /// when implementations or reference versions are updated.
    pub fn with_metadata(mut self, created_at: u64, version: impl Into<String>) -> Self {
        self.created_at = created_at;
        self.version = version.into();
        self
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Version is intentionally excluded: new tools follow older introductions.
    pub fn sort_key(&self) -> (u64, &str) {
        (self.created_at, &self.name)
    }

    /// 展示说明、版本和参数顺序不改变执行契约。所有参数目前都是字符串；
    /// 新增参数类型或值域约束时，也必须纳入此比较。
    pub fn same_contract(&self, other: &Self) -> bool {
        self.name == other.name
            && self.read_only == other.read_only
            && self.parameters.len() == other.parameters.len()
            && self
                .parameters
                .iter()
                .enumerate()
                .all(|(index, parameter)| {
                    !self.parameters[..index]
                        .iter()
                        .any(|previous| previous.name == parameter.name)
                        && other.parameters.iter().any(|current| {
                            parameter.name == current.name && parameter.required == current.required
                        })
                })
    }

    pub(crate) fn refresh_metadata(&mut self, current: &Self) {
        self.created_at = current.created_at;
        self.version.clone_from(&current.version);
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
}
