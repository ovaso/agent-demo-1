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
        }
    }

    pub fn text(actor: &'static str, text: &str) -> Self {
        Self {
            text: text.into(),
            ..Self::tools(actor, vec![])
        }
    }

    pub fn interrupted(mut self) -> Self {
        self.complete = false;
        self
    }

    fn response(&self, provider: Provider) -> String {
        let mut events = Vec::new();
        if !self.text.is_empty() {
            events.push(match provider {
                Provider::OpenAi => json!({"choices":[{"delta":{"content":self.text}}]}),
                Provider::Anthropic => json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":self.text}}),
            });
        }
        for (index, call) in self.calls.iter().enumerate() {
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
    assert_eq!(overview(&request)["actor"], step.actor);
    drop(reader);
    let response = step.response(provider);
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
    stream.flush().unwrap();
    request
}

pub fn overview(request: &Value) -> Value {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_str())
        .find_map(|text| text.strip_prefix("运行状态（数据）："))
        .map(|text| serde_json::from_str(text).unwrap())
        .expect("runtime state sent to provider")
}

pub fn has_tool(request: &Value, name: &str) -> bool {
    request["tools"].as_array().unwrap().iter().any(|tool| {
        tool["name"].as_str() == Some(name) || tool["function"]["name"].as_str() == Some(name)
    })
}
