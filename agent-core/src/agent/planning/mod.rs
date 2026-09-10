//! 有版本的任务计划。计划定义与执行进度分离。
use super::runtime::RuntimeError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_PLAN_TASKS: usize = 64;
pub const MAX_PLAN_VERSIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskAction {
    Agent {
        prompt: String,
    },
    Tool {
        name: String,
        arguments: BTreeMap<String, String>,
        check: ToolCheck,
    },
}

impl TaskAction {
    pub(crate) fn tool_call(&self, id: impl Into<String>) -> Option<crate::tool::ToolCall> {
        let Self::Tool {
            name, arguments, ..
        } = self
        else {
            return None;
        };
        let arguments = arguments
            .iter()
            .fold(crate::tool::Arguments::new(), |args, (key, value)| {
                args.with(key, value)
            });
        Some(crate::tool::ToolCall::new(id, name, arguments))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCheck {
    Succeeded,
    ExitCodeZero,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTask {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub acceptance: Vec<String>,
    pub action: TaskAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub goal: String,
    pub requirements: Vec<String>,
    pub tasks: Vec<PlanTask>,
}

impl Plan {
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.goal.trim().is_empty()
            || self.requirements.is_empty()
            || self.tasks.is_empty()
            || self.tasks.len() > MAX_PLAN_TASKS
        {
            return Err(RuntimeError::Invalid(
                "计划必须包含目标、验收要求和 1..=64 个任务".into(),
            ));
        }
        let size = serde_json::to_vec(self).map_err(RuntimeError::from)?.len();
        if size > 128 * 1024 {
            return Err(RuntimeError::Invalid("计划超过 128 KiB".into()));
        }
        let requirements: BTreeSet<_> = self.requirements.iter().collect();
        if requirements.len() != self.requirements.len()
            || requirements.iter().any(|value| value.trim().is_empty())
        {
            return Err(RuntimeError::Invalid("验收要求不能为空或重复".into()));
        }
        let mut remaining = BTreeMap::new();
        for task in &self.tasks {
            if task.id.is_empty()
                || task.id.len() > 128
                || task.id.contains(char::is_whitespace)
                || task.description.trim().is_empty()
                || task.acceptance.is_empty()
                || task.acceptance.iter().any(|s| s.trim().is_empty())
                || remaining
                    .insert(task.id.as_str(), task.depends_on.len())
                    .is_some()
            {
                return Err(RuntimeError::Invalid(
                    "任务标识重复或任务缺少描述、验收条件".into(),
                ));
            }
            match &task.action {
                TaskAction::Agent { prompt } if prompt.trim().is_empty() => {
                    return Err(RuntimeError::Invalid("Agent 任务缺少指令".into()));
                }
                TaskAction::Tool { name, .. } if name.trim().is_empty() => {
                    return Err(RuntimeError::Invalid("工具任务缺少工具名".into()));
                }
                _ => {}
            }
        }
        for task in &self.tasks {
            let dependencies: BTreeSet<_> = task.depends_on.iter().collect();
            if dependencies.len() != task.depends_on.len()
                || task
                    .depends_on
                    .iter()
                    .any(|id| !remaining.contains_key(id.as_str()))
            {
                return Err(RuntimeError::Invalid("任务依赖重复或不存在".into()));
            }
        }
        let mut ready: VecDeque<_> = remaining
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut visited = 0;
        while let Some(id) = ready.pop_front() {
            visited += 1;
            for task in &self.tasks {
                if task.depends_on.iter().any(|dep| dep == id) {
                    let degree = remaining.get_mut(task.id.as_str()).ok_or_else(|| {
                        RuntimeError::Invalid(format!("计划依赖计数缺少任务 {}", task.id))
                    })?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push_back(task.id.as_str());
                    }
                }
            }
        }
        if visited != self.tasks.len() {
            return Err(RuntimeError::Invalid("任务依赖存在环".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanHistory {
    versions: Vec<Plan>,
}

impl PlanHistory {
    pub fn revision(&self) -> u64 {
        self.versions.len() as u64
    }
    pub fn current(&self) -> Option<&Plan> {
        self.versions.last()
    }
    pub fn version(&self, revision: u64) -> Option<&Plan> {
        revision
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.versions.get(index))
    }
    pub fn propose(&mut self, expected_revision: u64, plan: Plan) -> Result<u64, RuntimeError> {
        if expected_revision != self.revision() {
            return Err(RuntimeError::Conflict);
        }
        if self.versions.len() >= MAX_PLAN_VERSIONS {
            return Err(RuntimeError::Invalid("重新规划次数达到上限".into()));
        }
        plan.validate()?;
        if let Some(previous) = self.current()
            && (previous.goal != plan.goal
                || previous
                    .requirements
                    .iter()
                    .any(|item| !plan.requirements.contains(item)))
        {
            return Err(RuntimeError::Invalid(
                "重新规划不得改变目标或删除已有验收要求".into(),
            ));
        }
        self.versions.push(plan);
        Ok(self.revision())
    }
}
