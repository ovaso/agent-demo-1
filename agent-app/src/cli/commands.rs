use agent_core::agent::routing::ExecutionMode;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command<'a> {
    Input(&'a str),
    Start(&'a str),
    Plan(Option<&'a str>),
    Execute(Option<&'a str>),
    Board(Option<&'a str>),
    Mode(Option<ExecutionMode>),
    Graph,
    RetryNode(&'a str),
    Agents,
    AgentBudget(&'a str, u64),
    CancelAgent(&'a str),
    Messages(Option<&'a str>),
    Message(&'a str, &'a str),
    Reply(&'a str, &'a str),
    Tokens(Option<u64>),
    OutputBudget(u64),
    Help,
    Trace,
    Reset,
    Exit,
    Status(Option<&'a str>),
    Resume(Option<&'a str>),
    Step(Option<&'a str>),
    Pause(Option<&'a str>),
    Cancel(Option<&'a str>),
    Budget(u64, Option<&'a str>),
    BudgetAuto(u64, Option<&'a str>),
    Resolve(&'a str, &'a str),
    Retry(&'a str),
}

pub(super) fn parse(input: &str) -> Result<Command<'_>, String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return Ok(Command::Input(input));
    }
    let (name, rest) = split(trimmed);
    match name {
        "/tokens" => Ok(Command::Tokens(if rest.is_empty() {
            None
        } else {
            Some(
                rest.parse()
                    .map_err(|_| "用法：/tokens [总额度，0 关闭]".to_string())?,
            )
        })),
        "/output-budget" => {
            let limit = rest
                .parse::<u64>()
                .map_err(|_| "用法：/output-budget <正整数>".to_string())?;
            if limit == 0 {
                return Err("输出上限必须大于零".into());
            }
            Ok(Command::OutputBudget(limit))
        }
        "/start" if !rest.is_empty() => Ok(Command::Start(rest)),
        "/plan" => Ok(Command::Plan(if rest.is_empty() {
            None
        } else {
            Some(rest)
        })),
        "/execute" => Ok(Command::Execute(optional_id(rest)?)),
        "/board" => Ok(Command::Board(optional_id(rest)?)),
        "/mode" => Ok(Command::Mode(match rest {
            "" => None,
            "loop" => Some(ExecutionMode::Loop),
            "graph" => Some(ExecutionMode::Graph),
            _ => return Err("用法：/mode [loop|graph]".into()),
        })),
        "/graph" if rest.is_empty() => Ok(Command::Graph),
        "/agents" if rest.is_empty() => Ok(Command::Agents),
        "/messages" => Ok(Command::Messages(optional_id(rest)?)),
        "/message" | "/reply" => {
            let (target, body) = split(rest);
            if target.is_empty() || body.is_empty() {
                return Err("用法：/message <地址> <内容> 或 /reply <请求ID> <答复>".into());
            }
            Ok(if name == "/message" {
                Command::Message(target, body)
            } else {
                Command::Reply(target, body)
            })
        }
        "/agent-budget" => {
            let (node, value) = split(rest);
            let max_steps = value
                .parse()
                .ok()
                .filter(|value| *value > 0)
                .ok_or("用法：/agent-budget <节点ID> <累计步数上限>")?;
            if node.is_empty() {
                return Err("缺少节点 ID".into());
            }
            Ok(Command::AgentBudget(node, max_steps))
        }
        "/cancel-agent" if !rest.is_empty() => {
            Ok(Command::CancelAgent(optional_id(rest)?.expect("nonempty")))
        }
        "/retry-node" if !rest.is_empty() => {
            Ok(Command::RetryNode(optional_id(rest)?.expect("nonempty")))
        }
        "/help" if rest.is_empty() => Ok(Command::Help),
        "/trace" if rest.is_empty() => Ok(Command::Trace),
        "/reset" if rest.is_empty() => Ok(Command::Reset),
        "/exit" | "/quit" if rest.is_empty() => Ok(Command::Exit),
        "/status" => Ok(Command::Status(optional_id(rest)?)),
        "/resume" => Ok(Command::Resume(optional_id(rest)?)),
        "/step" => Ok(Command::Step(optional_id(rest)?)),
        "/pause" => Ok(Command::Pause(optional_id(rest)?)),
        "/cancel" => Ok(Command::Cancel(optional_id(rest)?)),
        "/budget" => {
            if rest.is_empty() {
                return Ok(Command::Status(None));
            }
            let (value, id) = split(rest);
            if value == "auto" {
                let (ceiling, id) = split(id);
                let ceiling = ceiling
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or("用法：/budget auto <正整数硬上限> [运行 ID]")?;
                return Ok(Command::BudgetAuto(ceiling, optional_id(id)?));
            }
            let steps = value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or("用法：/budget <正整数总步数> [运行 ID]")?;
            Ok(Command::Budget(steps, optional_id(id)?))
        }
        "/resolve" => {
            let (call_id, content) = split(rest);
            if call_id.is_empty() || content.is_empty() {
                return Err("用法：/resolve <工具调用 ID> <已核实的结果>".into());
            }
            Ok(Command::Resolve(call_id, content))
        }
        "/retry" if !rest.is_empty() => Ok(Command::Retry(optional_id(rest)?.expect("nonempty"))),
        _ => Err(format!("未知命令或参数无效：{name}；输入 /help 查看用法")),
    }
}

fn split(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map(|(first, rest)| (first, rest.trim_start()))
        .unwrap_or((text, ""))
}

fn optional_id(text: &str) -> Result<Option<&str>, String> {
    if text.is_empty() {
        Ok(None)
    } else if text.contains(char::is_whitespace) {
        Err("此命令最多接受一个运行 ID".into())
    } else {
        Ok(Some(text))
    }
}
