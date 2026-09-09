#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command<'a> {
    Input(&'a str),
    Start(&'a str),
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
        "/start" if !rest.is_empty() => Ok(Command::Start(rest)),
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
            let (value, id) = split(rest);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_user_input_and_resolved_multiline_result() {
        assert_eq!(
            parse("  hello\nworld  ").unwrap(),
            Command::Input("  hello\nworld  ")
        );
        assert_eq!(
            parse("/resolve call-1 first\nsecond").unwrap(),
            Command::Resolve("call-1", "first\nsecond")
        );
    }

    #[test]
    fn parses_explicit_resume_and_rejects_ambiguous_or_invalid_control() {
        assert_eq!(
            parse("/resume run-1").unwrap(),
            Command::Resume(Some("run-1"))
        );
        assert_eq!(
            parse("/budget 12 run-1").unwrap(),
            Command::Budget(12, Some("run-1"))
        );
        for invalid in [
            "/budget 0",
            "/budget -1",
            "/resume a b",
            "/resolve a",
            "/retry",
            "/start",
            "/help ignored",
        ] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
    }
}
