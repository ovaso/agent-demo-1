use crate::trace_map;
use agent_core::agent::runtime::{LoopPhase, PauseReason, RunState, RunStatus};
use std::{error::Error, io};

pub(super) fn print_status(state: &RunState) {
    println!(
        "执行方式：{:?}；待切换：{:?}；活动节点：{:?}",
        state.routing().mode(),
        state.routing().pending_mode(),
        state.graph().active_node()
    );
    println!(
        "运行：{}\n状态：{:?}\n阶段：{:?}\n模型步数：{}/{}；工具调用：{}/{}；检查点：{}",
        state.id(),
        state.status(),
        state.phase(),
        state.budget().model_calls(),
        state.limits().max_steps,
        state.budget().tool_calls(),
        state.limits().max_tool_calls,
        state.revision()
    );
    print_budget(state);
    if let Some(result) = state.result() {
        println!("结果：{}", result.text());
    }
    if state.status() == &RunStatus::Paused(agent_core::agent::runtime::PauseReason::PlanReady) {
        print_plan(state);
        println!("计划已保存；/execute 开始执行，/resume 保持只规划边界。");
    }
    if state.status() == &RunStatus::Paused(PauseReason::Budget) {
        let reason = if state.budget().transitions() >= state.limits().max_transitions {
            "已达到状态转换次数上限"
        } else if state.phase() == &LoopPhase::Tools
            && state.budget().tool_calls() >= state.limits().max_tool_calls
        {
            "已达到工具调用次数上限"
        } else if state.budget().model_calls() < state.limits().max_steps {
            "额度已调整，等待 /resume 恢复"
        } else {
            state
                .step_extension_block()
                .map_or("已满足续期条件，等待 /resume 恢复", |reason| {
                    reason.description()
                })
        };
        println!("暂停原因：{reason}。进度已保存在检查点中，/resume 不会重置额度。");
        if state.phase() == &LoopPhase::Model
            && state.budget().transitions() < state.limits().max_transitions
            && state.budget().model_calls() >= state.limits().max_steps
            && state.step_extension_block().is_some()
        {
            if state.limits().step_extension.is_none() {
                println!("可用 /budget auto <硬上限> 启用有限续期，再 /resume 继续原任务。");
            }
            println!(
                "需要额外执行时，可用 /budget <更高的固定总额度> 明确授权，再 /resume；已用额度和续期历史不会清零。"
            );
        }
    }
}

pub(super) fn print_budget(state: &RunState) {
    if let Some(policy) = &state.limits().step_extension {
        println!(
            "预算策略：已获准 {} 步，硬上限 {} 步；自动续期 {}/{} 次，每次最多 {} 步。",
            state.limits().max_steps,
            policy.hard_max_steps,
            state.budget().step_extensions().len(),
            policy.max_extensions,
            policy.step_increment
        );
    } else {
        println!(
            "预算策略：固定总额度 {} 步，自动续期未启用。",
            state.limits().max_steps
        );
    }
    for (index, grant) in state.budget().step_extensions().iter().enumerate() {
        println!(
            "预算续期 {}：{} → {}，发生于已用 {} 步时，新增进展 {} 项。",
            index + 1,
            grant.previous_limit,
            grant.granted_limit,
            grant.at_model_call,
            grant.new_progress
        );
    }
}

pub(super) fn print_graph(state: &RunState) {
    if let Some(graph) = state.graph().current() {
        println!(
            "工作节点，计划版本 {}，图调度启用：{}",
            graph.plan_version,
            graph.is_engaged()
        );
        for (id, node) in &graph.nodes {
            if !graph.is_engaged()
                && node.origin == agent_core::agent::delegation::NodeOrigin::Planned
            {
                continue;
            }
            println!(
                "  [{id}] {:?}，尝试 {}，验收依据 {:?}\n    {}",
                node.status, node.attempts, node.validation, node.task.description
            );
        }
    } else {
        println!("尚未启动图执行。");
    }
}

pub(super) fn print_agents(state: &RunState) {
    if let Some(graph) = state.graph().current() {
        for (id, node) in &graph.nodes {
            if let Some(policy) = &node.policy {
                println!(
                    "  {id}：{:?}，模型步数 {}/{}，业务工具范围 {:?}",
                    node.status, policy.model_calls, policy.max_steps, policy.tools
                );
            }
        }
    }
}

pub(super) fn print_messages(state: &RunState, id: Option<&str>) -> Result<(), Box<dyn Error>> {
    if let Some(id) = id {
        let message = state.collaboration().get(id).ok_or("找不到消息 ID")?;
        println!("{}", serde_json::to_string_pretty(message)?);
    } else {
        for message in state.collaboration().messages() {
            println!(
                "{}  {} → {}  {}  计划v{}",
                message.id,
                message.from,
                message.to,
                message.status.label(),
                message.plan_version
            );
        }
    }
    Ok(())
}

pub(super) fn print_plan(state: &RunState) {
    match state.plans().current() {
        Some(plan) => {
            println!("计划 v{}：{}", state.plans().revision(), plan.goal);
            println!("总体要求：{}", plan.requirements.join("；"));
            for task in &plan.tasks {
                println!(
                    "  [{}] {}\n    依赖：{}；验收：{}",
                    task.id,
                    task.description,
                    task.depends_on.join(", "),
                    task.acceptance.join("；")
                );
            }
        }
        None => println!("尚未提交结构化计划。"),
    }
}

pub(super) fn help() {
    println!(
        "命令：
  /start <任务>          建立任务，随后可用 /step 逐步执行
  /plan <任务>          只调查和规划，不执行写入
  /plan                 查看当前计划
  /execute [运行 ID]    执行已交付的计划，沿用原预算
  /board [记录标识]     查看共享记录（未指定时最多 32 条）
  /mode [loop|graph]    查看或选择执行方式
  /graph               查看图节点状态
  /retry-node <节点ID>  明确重试已知失败节点，随后 /resume
  /agents              查看子 Agent 状态、工具范围与预算
  /agent-budget <ID> <步数>  调整子 Agent 累计额度
  /cancel-agent <ID>   取消子任务，保留记录和用量
  /messages [消息ID]  查看协作消息状态或内容
  /message <地址> <内容>  给 main 或 node/节点ID 留下通知
  /reply <请求ID> <答复>  以操作者身份答复，再 /resume
  /status [运行 ID]     查看状态、预算和最终结果
  /resume [运行 ID]     恢复执行，沿用原预算
  /step [运行 ID]       推进一个阶段（暂停后需 /resume）
  /pause [运行 ID]      暂停待执行任务
  /cancel [运行 ID]     取消任务，保留记录
  /budget               查看预算、续期记录和当前状态
  /budget auto <硬上限> [ID] 启用有限自动续期，保留用量和历史
  /budget <总步数> [ID] 设置固定总额度并关闭自动续期
  /resolve <调用 ID> <结果>  提交已核实的工具结果
  /retry <调用 ID>      明确重试结果未知的工具
  /trace               查看调用树
  /reset               清除已结束任务的会话历史
  /exit                退出（未结束任务可恢复）
普通文本直接开始并执行任务；控制命令省略 ID 时使用当前会话最新任务。
Enter 发送，Ctrl+J / Alt+Enter 换行；Ctrl+C 取消输入，Ctrl+D 退出。"
    );
}

pub(super) fn show_trace(path: &str) -> Result<(), Box<dyn Error>> {
    trace_map::show(path, &mut io::stdout().lock())?;
    Ok(())
}
