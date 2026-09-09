//! 离线读取调用链，按开始顺序显示父子关系；不加载原始提示词和工具输出。
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
};

use serde_json::Value;

const MAX_LINE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;

struct Node {
    depth: usize,
    label: String,
    offset: f64,
    finish: Option<Value>,
    first_delta_ms: Option<u64>,
}

pub(crate) fn show(path: impl AsRef<Path>, output: &mut impl Write) -> io::Result<()> {
    render(BufReader::new(File::open(path)?), output)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn render(mut input: impl BufRead, output: &mut impl Write) -> io::Result<()> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut lookup = BTreeMap::new();
    let mut runs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut run_order = Vec::new();
    let mut line = Vec::new();
    let mut metadata_bytes = 0usize;
    let mut line_number = 0;
    loop {
        line.clear();
        let length = input
            .by_ref()
            .take(MAX_LINE_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if length == 0 {
            break;
        }
        line_number += 1;
        if length as u64 > MAX_LINE_BYTES {
            return Err(invalid(format!(
                "追踪第 {line_number} 行超过 4 MiB，请拆分日志"
            )));
        }
        let event: Value = match serde_json::from_slice(&line) {
            Ok(event) => event,
            Err(_) if line.last() != Some(&b'\n') => {
                writeln!(
                    output,
                    "注意：忽略末尾不完整记录，未结束节点将显示 incomplete。"
                )?;
                break;
            }
            Err(error) => return Err(invalid(format!("追踪第 {line_number} 行无效：{error}"))),
        };
        let name = event["name"].as_str().unwrap_or_default();
        if !matches!(
            name,
            "trace.span.started"
                | "trace.span.completed"
                | "trace.span.failed"
                | "model.first_text_delta"
        ) {
            continue;
        }
        let fields = &event["fields"];
        // 旧格式首字延迟事件没有关联 ID，仍可跳过。
        if name == "model.first_text_delta" && fields["run_id"].is_null() {
            continue;
        }
        let run = fields["run_id"]
            .as_str()
            .ok_or_else(|| invalid("节点缺少 run_id"))?;
        let id = fields["span_id"]
            .as_u64()
            .ok_or_else(|| invalid("节点缺少 span_id"))?;
        metadata_bytes = metadata_bytes.saturating_add(length);
        if metadata_bytes > MAX_METADATA_BYTES {
            return Err(invalid("调用链元数据超过 64 MiB，请拆分日志"));
        }
        let key = (run.to_owned(), id);
        if name == "trace.span.started" {
            if nodes.len() >= MAX_NODES {
                return Err(invalid("调用节点超过 100000，请拆分日志"));
            }
            if lookup.contains_key(&key) {
                return Err(invalid("重复的调用节点 ID"));
            }
            let depth = fields["depth"]
                .as_u64()
                .filter(|depth| *depth <= 64)
                .ok_or_else(|| invalid("调用深度无效或超过 64"))? as usize;
            if let Some(parent) = fields["parent_span_id"].as_u64() {
                let index: &usize = lookup
                    .get(&(run.to_owned(), parent))
                    .ok_or_else(|| invalid("父调用节点不存在"))?;
                if nodes[*index].depth + 1 != depth {
                    return Err(invalid("调用深度与父节点不一致"));
                }
            } else if depth != 0 {
                return Err(invalid("非根节点缺少父节点"));
            }
            let index = nodes.len();
            let label = format!(
                "{} {}",
                fields["operation"]
                    .as_str()
                    .unwrap_or("unknown")
                    .escape_debug(),
                fields["details"]
            );
            nodes.push(Node {
                depth,
                label,
                offset: fields["start_offset_ms"].as_f64().unwrap_or(0.0),
                finish: None,
                first_delta_ms: None,
            });
            lookup.insert(key, index);
            if !runs.contains_key(run) {
                run_order.push((
                    run.to_owned(),
                    fields["session_id"].as_str().unwrap_or_default().to_owned(),
                ));
            }
            runs.entry(run.to_owned()).or_default().push(index);
        } else {
            let index = lookup
                .get(&key)
                .ok_or_else(|| invalid("结束事件找不到开始节点"))?;
            if name == "model.first_text_delta" {
                nodes[*index].first_delta_ms = fields["latency_ms"].as_u64();
            } else {
                if nodes[*index].finish.is_some() {
                    return Err(invalid("节点重复结束"));
                }
                nodes[*index].finish = Some(fields.clone());
            }
        }
    }
    if nodes.is_empty() {
        return writeln!(
            output,
            "没有调用链节点；旧版事件无法还原完整调用树，请运行一次新版 Agent。"
        );
    }
    writeln!(
        output,
        "调用树（按开始顺序；耗时包含子调用，token 为子树汇总，勿跨层相加）"
    )?;
    for (run, session) in run_order {
        writeln!(
            output,
            "\nrun={} session={}",
            run.escape_debug(),
            session.escape_debug()
        )?;
        for index in &runs[&run] {
            write_node(&nodes[*index], output)?;
        }
    }
    Ok(())
}

fn count(value: &Value) -> String {
    value
        .as_u64()
        .map_or_else(|| "?".to_owned(), |value| value.to_string())
}

fn write_node(node: &Node, output: &mut impl Write) -> io::Result<()> {
    write!(
        output,
        "{}{} [+{:.3}ms]",
        "  ".repeat(node.depth),
        node.label,
        node.offset
    )?;
    let Some(fields) = &node.finish else {
        return writeln!(output, " incomplete 耗时=? token=? 缓存=unknown");
    };
    let usage = &fields["usage"];
    write!(
        output,
        " {} {:.3}ms token(in/out/total)={}/{}/{} cache={} read={} write={} hits={}/{} calls={} reasoning={}",
        fields["status"].as_str().unwrap_or("unknown"),
        fields["duration_ms"].as_f64().unwrap_or(0.0),
        count(&usage["input_tokens"]),
        count(&usage["output_tokens"]),
        count(&fields["total_tokens"]),
        fields["cache_status"].as_str().unwrap_or("unknown"),
        count(&usage["cached_input_tokens"]),
        count(&usage["cache_write_input_tokens"]),
        count(&fields["cache_hits"]),
        count(&fields["cache_reports"]),
        count(&fields["model_calls"]),
        count(&usage["reasoning_tokens"])
    )?;
    if let Some(latency) = node.first_delta_ms {
        write!(output, " 首字={latency}ms")?;
    }
    if let Some(error) = fields["error"].as_str() {
        write!(output, " error={}", error.escape_debug())?;
    }
    writeln!(output)
}

#[cfg(test)]
mod tests;
