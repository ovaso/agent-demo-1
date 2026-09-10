use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    time::Duration,
};

#[derive(Clone, Copy)]
pub enum Provider {
    OpenAi,
    Anthropic,
}

pub struct Step {
    pub actor: &'static str,
    text: String,
    calls: Vec<Value>,
    pub complete: bool,
    stop: Option<&'static str>,
    reasoning: Option<&'static str>,
}

pub fn call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id":id,"name":name,"arguments":arguments})
}

impl Step {
    pub fn tools(actor: &'static str, calls: Vec<Value>) -> Self {
        Self {
            actor,
            text: String::new(),
            calls,
            complete: true,
            stop: None,
            reasoning: None,
        }
    }

    pub fn text(actor: &'static str, text: &str) -> Self {
        Self {
            text: text.into(),
            ..Self::tools(actor, vec![])
        }
    }

    pub fn reasoning(mut self, text: &'static str) -> Self {
        self.reasoning = Some(text);
        self
    }

    pub fn stopped(mut self, reason: &'static str) -> Self {
        self.stop = Some(reason);
        self
    }

    pub fn interrupted(mut self) -> Self {
        self.complete = false;
        self
    }

    fn response(&self, provider: Provider) -> String {
        let mut events = Vec::new();
        let offset = usize::from(self.reasoning.is_some());
        if let Some(thinking) = self.reasoning {
            match provider {
                Provider::OpenAi => events.push(json!({"model":"reported-model","choices":[{"delta":{"reasoning_content":thinking}}]})),
                Provider::Anthropic => {
                    events.push(json!({"type":"message_start","message":{"model":"reported-model"}}));
                    events.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}));
                    events.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":thinking}}));
                    events.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic-signature"}}));
                    events.push(json!({"type":"content_block_stop","index":0}));
                }
            }
        }
        if !self.text.is_empty() {
            events.push(match provider {
                Provider::OpenAi => json!({"choices":[{"delta":{"content":self.text}}]}),
                Provider::Anthropic => json!({"type":"content_block_delta","index":offset,"delta":{"type":"text_delta","text":self.text}}),
            });
        }
        for (index, call) in self.calls.iter().enumerate() {
            let index = index + offset + usize::from(!self.text.is_empty());
            let args = call["arguments"].to_string();
            let mut middle = args.len() / 2;
            while !args.is_char_boundary(middle) {
                middle -= 1;
            }
            let (first, second) = args.split_at(middle);
            match provider {
                Provider::OpenAi => {
                    events.push(json!({"choices":[{"delta":{"tool_calls":[{"index":index,"id":call["id"],"type":"function","function":{"name":call["name"],"arguments":first}}]}}]}));
                    events.push(json!({"choices":[{"delta":{"tool_calls":[{"index":index,"function":{"arguments":second}}]}}]}));
                }
                Provider::Anthropic => {
                    events.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":call["id"],"name":call["name"],"input":{}}}));
                    for part in [first, second] {
                        events.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":part}}));
                    }
                    events.push(json!({"type":"content_block_stop","index":index}));
                }
            }
        }
        if let Some(reason) = self.stop {
            events.push(match provider {
                Provider::OpenAi => json!({"choices":[{"delta":{},"finish_reason":reason}]}),
                Provider::Anthropic => {
                    json!({"type":"message_delta","delta":{"stop_reason":reason}})
                }
            });
        }
        let mut body: String = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect();
        if self.complete {
            body.push_str(match provider {
                Provider::OpenAi => "data: [DONE]\n\n",
                Provider::Anthropic => "data: {\"type\":\"message_stop\"}\n\n",
            });
        }
        body
    }
}

pub fn exchange(mut stream: TcpStream, provider: Provider, step: &Step) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let path = match provider {
        Provider::OpenAi => "/v1/chat/completions",
        Provider::Anthropic => "/v1/messages",
    };
    assert_eq!(line.trim(), format!("POST {path} HTTP/1.1"));
    let mut length = None;
    let mut authenticated = false;
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
        authenticated |= match provider {
            Provider::OpenAi => {
                name.eq_ignore_ascii_case("authorization") && value.trim() == "Bearer test-key"
            }
            Provider::Anthropic => {
                name.eq_ignore_ascii_case("x-api-key") && value.trim() == "test-key"
            }
        };
    }
    assert!(authenticated);
    let length = length.expect("JSON request needs content-length");
    assert!(length <= 4 * 1024 * 1024);
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(request["model"], "test-model");
    assert_eq!(request["stream"], true);
    let tool_names: Vec<_> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .or_else(|| tool["function"]["name"].as_str())
                .unwrap()
        })
        .collect();
    assert!(
        tool_names
            .windows(2)
            .all(|names| tool_sort_key(names[0]) < tool_sort_key(names[1])),
        "tool definitions must be sorted by introduction and name: {tool_names:?}"
    );
    assert_eq!(overview(&request)["actor"], step.actor);
    drop(reader);
    let response = step.response(provider);
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
    stream.flush().unwrap();
    request
}

pub fn overview(request: &Value) -> Value {
    let mut state = serde_json::Map::new();
    for text in request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
    {
        let json = if let Some(value) = text.strip_prefix("运行状态（数据）：") {
            state.clear();
            value
        } else if let Some(value) = text.strip_prefix("运行状态增量（数据）：") {
            value
        } else {
            continue;
        };
        let value: Value = serde_json::from_str(json).unwrap();
        state.extend(value.as_object().unwrap().clone());
    }
    Value::Object(state)
}

pub fn has_tool(request: &Value, name: &str) -> bool {
    request["tools"].as_array().unwrap().iter().any(|tool| {
        tool["name"].as_str() == Some(name) || tool["function"]["name"].as_str() == Some(name)
    })
}

fn tool_sort_key(name: &str) -> (u64, &str) {
    let created_at = match name {
        "echo" | "session_finish" => 1788784117,
        "run_cmd" | "write_file" => 1788951445,
        "list_directory"
        | "read_file"
        | "search_files"
        | "runtime_plan"
        | "runtime_plan_ready"
        | "runtime_board_read"
        | "runtime_board_write" => 1789006513,
        "run_check" | "runtime_route" | "runtime_run_node" | "runtime_retry_node" => 1789008818,
        "runtime_delegate"
        | "runtime_agents"
        | "runtime_agent_budget"
        | "runtime_cancel_agent"
        | "runtime_result" => 1789010818,
        "runtime_send" | "runtime_ask" | "runtime_reply" | "runtime_wait" | "runtime_inbox" => {
            1789013643
        }
        "debug_show_config" => 1789028730,
        _ => panic!("add the new tool's introduction date to this contract: {name}"),
    };
    (created_at, name)
}
