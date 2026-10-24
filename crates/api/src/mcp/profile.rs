//! 출처 파일(`*.mcp.json`)의 bytes에 요청한 연결 정의가 그대로 있는지 본다. 화면이 파일을 읽어 만든 요청과
//! 데몬이 확인한 파일이 다르면(화면 결함·경합) 실행하지 않는다. 스키마는 화면 TS 소유(`mcp/session.ts`)이고
//! 데몬은 최상위 `transport`의 `command`·`args`·`cwd`·`env`(stdio)와 `url`·`headers`(HTTP)만 비교한다.
use super::McpTransport;
use serde_json::Value;

fn strings(value: Option<&Value>) -> Option<Vec<&str>> {
    match value {
        None | Some(Value::Null) => Some(vec![]),
        Some(Value::Array(items)) => items.iter().map(Value::as_str).collect(),
        Some(_) => None,
    }
}

/// `[{name, value, enabled?}]` 중 켜진 행.
fn pairs(value: Option<&Value>) -> Option<Vec<(&str, &str)>> {
    let items = match value {
        None | Some(Value::Null) => return Some(vec![]),
        Some(Value::Array(items)) => items,
        Some(_) => return None,
    };
    let mut out = vec![];
    for item in items {
        if item.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        out.push((item.get("name")?.as_str()?, item.get("value")?.as_str()?));
    }
    Some(out)
}

fn same_pairs(mut a: Vec<(&str, &str)>, mut b: Vec<(&str, &str)>) -> bool {
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

fn object_matches(map: &serde_json::Map<String, Value>, transport: &McpTransport) -> bool {
    match transport {
        McpTransport::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            map.get("command").and_then(Value::as_str) == Some(command.as_str())
                && strings(map.get("args"))
                    .is_some_and(|a| a == args.iter().map(String::as_str).collect::<Vec<_>>())
                && match map.get("cwd") {
                    None | Some(Value::Null) => cwd.is_none(),
                    Some(Value::String(c)) => cwd.as_deref() == Some(c.as_str()),
                    Some(_) => false,
                }
                && pairs(map.get("env")).is_some_and(|file| {
                    same_pairs(
                        file,
                        env.iter()
                            .map(|p| (p.name.as_str(), p.value.as_str()))
                            .collect(),
                    )
                })
        }
        McpTransport::Http { url, headers, .. } => {
            map.get("url").and_then(Value::as_str) == Some(url.as_str())
                && pairs(map.get("headers")).is_some_and(|file| {
                    same_pairs(
                        file,
                        headers
                            .iter()
                            .map(|h| (h.name.as_str(), h.value.as_str()))
                            .collect(),
                    )
                })
        }
    }
}

/// 화면이 보여 주는 최상위 `transport` 객체만 비교한다(다른 곳에 숨긴 같은 모양은 쓰지 않음, 검토 1 N2).
pub(crate) fn matches(bytes: &[u8], transport: &McpTransport) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok_and(|value| {
        value
            .get("transport")
            .and_then(Value::as_object)
            .is_some_and(|map| object_matches(map, transport))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pair;

    fn stdio(
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: &[(&str, &str)],
    ) -> McpTransport {
        McpTransport::Stdio {
            command: command.into(),
            args: args.iter().map(|a| (*a).into()).collect(),
            cwd: cwd.map(Into::into),
            env: env
                .iter()
                .map(|(n, v)| Pair {
                    name: (*n).into(),
                    value: (*v).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn only_the_exact_stdio_definition_matches() {
        let file = br#"{"version":1,"kind":"mcp","transport":{"kind":"stdio","command":"node",
            "args":["server.js","--x"],"cwd":"/p","env":[{"name":"K","value":"{{secret:K}}"}]}}"#;
        assert!(matches(
            file,
            &stdio(
                "node",
                &["server.js", "--x"],
                Some("/p"),
                &[("K", "{{secret:K}}")]
            )
        ));
        for other in [
            stdio(
                "bash",
                &["server.js", "--x"],
                Some("/p"),
                &[("K", "{{secret:K}}")],
            ),
            stdio("node", &["server.js"], Some("/p"), &[("K", "{{secret:K}}")]),
            stdio(
                "node",
                &["server.js", "--x", "-e"],
                Some("/p"),
                &[("K", "{{secret:K}}")],
            ),
            stdio(
                "node",
                &["server.js", "--x"],
                None,
                &[("K", "{{secret:K}}")],
            ),
            stdio(
                "node",
                &["server.js", "--x"],
                Some("/q"),
                &[("K", "{{secret:K}}")],
            ),
            stdio(
                "node",
                &["server.js", "--x"],
                Some("/p"),
                &[("K", "{{secret:OTHER}}")],
            ),
            stdio("node", &["server.js", "--x"], Some("/p"), &[]),
        ] {
            assert!(!matches(file, &other), "{other:?}");
        }
        assert!(!matches(b"{broken", &stdio("node", &[], None, &[])));
    }

    #[test]
    fn only_the_top_level_transport_is_compared() {
        let wanted = stdio("node", &["server.js"], None, &[]);
        let def = r#"{"kind":"stdio","command":"node","args":["server.js"],"cwd":null,"env":[]}"#;
        // 화면이 보여 주는 최상위 `transport`와 같아야 한다. 다른 곳에 숨긴 같은 정의는 쓰지 않는다.
        for hidden in [
            format!(
                r#"{{"version":1,"kind":"mcp","transport":{{"kind":"stdio","command":"echo","args":[],"cwd":null,"env":[]}},"x":{def}}}"#
            ),
            format!(r#"{{"version":1,"kind":"mcp","notes":[{{"transport":{def}}}]}}"#),
            format!(r#"{{"version":1,"kind":"mcp","transport":{{"inner":{def}}}}}"#),
            def.to_string(),
        ] {
            assert!(!matches(hidden.as_bytes(), &wanted), "{hidden}");
        }
        let shown = format!(r#"{{"version":1,"kind":"mcp","name":"srv","transport":{def}}}"#);
        assert!(matches(shown.as_bytes(), &wanted));
    }
}
