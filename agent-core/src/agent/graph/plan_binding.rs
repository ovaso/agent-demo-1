use super::{GraphRun, GraphState, NodeRun, NodeStatus};
use crate::agent::{
    planning::{Plan, TaskAction, ToolCheck},
    runtime::RuntimeError,
};
use std::collections::BTreeMap;

impl GraphState {
    pub(crate) fn bind(
        &mut self,
        version: u64,
        plan: &Plan,
        work_revision: u64,
    ) -> Result<(), RuntimeError> {
        if self
            .current()
            .is_some_and(|run| run.plan_version == version)
        {
            return Ok(());
        }
        if self.active.is_some() {
            return Err(RuntimeError::Invalid(
                "活动节点未结算，不能替换图计划".into(),
            ));
        }
        if self.has_open_delegations() {
            return Err(RuntimeError::Invalid(
                "修订计划前需完成或取消现有委托".into(),
            ));
        }
        plan.validate()?;
        if self.runs.len() >= 16 {
            return Err(RuntimeError::Invalid("图执行计划版本达到上限".into()));
        }
        // Reuse an identical successful task only when its prerequisites are reused.
        // Verification commands always run again after replanning.
        let mut nodes: BTreeMap<String, NodeRun> = plan
            .tasks
            .iter()
            .map(|task| (task.id.clone(), NodeRun::new(task.clone())))
            .collect();
        if let Some(previous) = self.current() {
            for (id, node) in &mut nodes {
                if let Some(old) = previous.nodes.get(id) {
                    node.prior_output.clone_from(&old.output);
                    if old.task == node.task
                        && (matches!(old.status, NodeStatus::Paused | NodeStatus::Failed)
                            || old.superseded)
                    {
                        *node = old.clone();
                        if node.superseded {
                            node.status = NodeStatus::Paused;
                            node.superseded = false;
                        }
                    }
                }
            }
            for _ in 0..nodes.len() {
                let reusable: Vec<_> = nodes
                    .iter()
                    .filter_map(|(id, node)| {
                        let old = previous.nodes.get(id)?;
                        (node.status == NodeStatus::Pending
                            && old.status == NodeStatus::Succeeded
                            && old.task == node.task
                            && ((!matches!(node.task.action, TaskAction::Agent { .. })
                                && !old.read_only)
                                || old.evidence_revision == work_revision)
                            && !matches!(
                                node.task.action,
                                TaskAction::Tool {
                                    check: ToolCheck::ExitCodeZero,
                                    ..
                                }
                            )
                            && node.task.depends_on.iter().all(|id| {
                                nodes
                                    .get(id)
                                    .is_some_and(|dep| dep.status == NodeStatus::Succeeded)
                            }))
                        .then(|| id.clone())
                    })
                    .collect();
                if reusable.is_empty() {
                    break;
                }
                for id in reusable {
                    let old = &previous.nodes[&id];
                    let node = nodes.get_mut(&id).expect("reused node");
                    node.status = old.status;
                    node.attempts = old.attempts;
                    node.output.clone_from(&old.output);
                    node.validation = old.validation;
                    node.evidence_revision = old.evidence_revision;
                    node.read_only = old.read_only;
                    node.policy = old.policy.clone();
                    node.reused_from = Some(previous.plan_version);
                    node.prior_output = String::new();
                }
            }
        }
        if let Some(previous) = self.current() {
            for (id, old) in &previous.nodes {
                if old.origin == crate::agent::delegation::NodeOrigin::Delegated
                    && !nodes.contains_key(id)
                {
                    let mut node = NodeRun::new(old.task.clone());
                    node.origin = old.origin;
                    node.policy = old.policy.clone();
                    node.status = old.status;
                    node.output.clone_from(&old.output);
                    node.validation = old.validation;
                    node.attempts = old.attempts;
                    node.reused_from = Some(previous.plan_version);
                    nodes.insert(id.clone(), node);
                }
            }
        }
        self.runs.push(GraphRun {
            engaged: self.current().is_some_and(|run| run.engaged),
            plan_version: version,
            nodes,
        });
        Ok(())
    }
}
