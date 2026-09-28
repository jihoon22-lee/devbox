//! The capture JSONPath subset; input and traversal have fixed limits.
use serde_json::Value;
const INVALID: &str = "capture_path_invalid";
const LIMIT: &str = "capture_path_limit";
enum Step {
    Member(String),
    Index(i64),
    Wildcard,
    Descend(String),
}

fn parse(path: &str) -> Result<Vec<Step>, &'static str> {
    if path.encode_utf16().count() > 256 || !path.starts_with('$') {
        return Err(INVALID);
    }
    let bytes = path.as_bytes();
    let mut i = 1;
    let mut steps = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                let descend = bytes.get(i) == Some(&b'.');
                if descend {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'*') {
                    steps.push(if descend {
                        Step::Descend("*".into())
                    } else {
                        Step::Wildcard
                    });
                    i += 1;
                    continue;
                }
                let start = i;
                if !bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'$'))
                {
                    return Err(INVALID);
                }
                i += 1;
                while bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'-'))
                {
                    i += 1;
                }
                let name = path[start..i].to_owned();
                steps.push(if descend {
                    Step::Descend(name)
                } else {
                    Step::Member(name)
                });
            }
            b'[' => {
                i += 1;
                if let Some(&quote @ (b'\'' | b'"')) = bytes.get(i) {
                    i += 1;
                    // Normalize quote style, preserving JSON escapes including surrogate pairs.
                    let mut quoted = String::from("\"");
                    loop {
                        let byte = *bytes.get(i).ok_or(INVALID)?;
                        i += 1;
                        if byte == quote {
                            break;
                        }
                        if byte == b'\\' {
                            let escaped = *bytes.get(i).ok_or(INVALID)?;
                            i += 1;
                            if escaped == quote && quote == b'\'' {
                                quoted.push('\'');
                            } else if escaped == b'"'
                                || escaped == b'\\'
                                || escaped == b'/'
                                || matches!(escaped, b'b' | b'f' | b'n' | b'r' | b't' | b'u')
                            {
                                quoted.push('\\');
                                quoted.push(char::from(escaped));
                                if escaped == b'u' {
                                    for _ in 0..4 {
                                        let hex = *bytes.get(i).ok_or(INVALID)?;
                                        if !hex.is_ascii_hexdigit() {
                                            return Err(INVALID);
                                        }
                                        quoted.push(char::from(hex));
                                        i += 1;
                                    }
                                }
                            } else {
                                return Err(INVALID);
                            }
                        } else {
                            if byte < 32 {
                                return Err(INVALID);
                            }
                            let character = path[i - 1..].chars().next().ok_or(INVALID)?;
                            if character == '"' {
                                quoted.push('\\');
                            }
                            quoted.push(character);
                            i += character.len_utf8() - 1;
                        }
                    }
                    quoted.push('"');
                    let name = serde_json::from_str::<String>(&quoted).map_err(|_| INVALID)?;
                    if bytes.get(i) != Some(&b']') {
                        return Err(INVALID);
                    }
                    i += 1;
                    steps.push(Step::Member(name));
                } else {
                    let start = i;
                    while bytes.get(i).is_some_and(|b| *b != b']') {
                        i += 1;
                    }
                    if bytes.get(i) != Some(&b']') {
                        return Err(INVALID);
                    }
                    let value = &path[start..i];
                    i += 1;
                    if value == "*" {
                        steps.push(Step::Wildcard);
                    } else {
                        let digits = value.strip_prefix('-').unwrap_or(value);
                        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                            return Err(INVALID);
                        }
                        let index: i64 = value.parse().map_err(|_| INVALID)?;
                        if index.unsigned_abs() > 9_007_199_254_740_991 {
                            return Err(INVALID);
                        }
                        steps.push(Step::Index(index));
                    }
                }
            }
            _ => return Err(INVALID),
        }
    }
    Ok(steps)
}
fn visit(count: &mut usize) -> Result<(), &'static str> {
    *count += 1;
    if *count > 10_000 {
        Err(LIMIT)
    } else {
        Ok(())
    }
}
fn children<'a>(value: &'a Value, count: &mut usize) -> Result<Vec<&'a Value>, &'static str> {
    let mut values = Vec::new();
    match value {
        Value::Array(items) => {
            for item in items {
                visit(count)?;
                values.push(item);
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                visit(count)?;
                values.push(item);
            }
        }
        _ => {}
    }
    Ok(values)
}
fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => object.get(name),
        Value::Array(items) => name
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == name)
            .and_then(|index| items.get(index)),
        _ => None,
    }
}
pub(crate) fn evaluate<'a>(value: &'a Value, path: &str) -> Result<Vec<&'a Value>, &'static str> {
    let mut current = vec![value];
    let mut visited = 0;
    for step in parse(path)? {
        let mut next = Vec::new();
        for node in current {
            visit(&mut visited)?;
            match &step {
                Step::Member(name) => next.extend(member(node, name)),
                Step::Index(index) => {
                    if let Value::Array(items) = node {
                        let position = if *index < 0 {
                            items.len() as i64 + index
                        } else {
                            *index
                        };
                        if position >= 0 {
                            next.extend(items.get(position as usize));
                        }
                    }
                }
                Step::Wildcard => next.extend(children(node, &mut visited)?),
                Step::Descend(name) => {
                    let mut pending = vec![node];
                    while let Some(item) = pending.pop() {
                        visit(&mut visited)?;
                        let nested = children(item, &mut visited)?;
                        if name == "*" {
                            next.extend(nested.iter().copied());
                        } else {
                            next.extend(member(item, name));
                        }
                        pending.extend(nested.into_iter().rev());
                    }
                }
            }
        }
        current = next;
    }
    Ok(current)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_members_indices_wildcards_and_recursive_members() {
        let doc = json!({"data":{"items":[{"id":1},{"id":2}],"odd key":true},"token":"t"});
        for (path, expected) in [
            ("$.token", vec![json!("t")]),
            ("$.data.items[-1].id", vec![json!(2)]),
            ("$.data.items[*].id", vec![json!(1), json!(2)]),
            ("$['data']['odd key']", vec![json!(true)]),
            ("$..id", vec![json!(1), json!(2)]),
            ("$.missing.deep", vec![]),
        ] {
            assert_eq!(
                evaluate(&doc, path).unwrap(),
                expected.iter().collect::<Vec<_>>(),
                "{path}"
            );
        }
    }

    #[test]
    fn rejects_scripts_invalid_escapes_and_oversized_traversal() {
        for path in [
            "token",
            "$.",
            "$[",
            "$..",
            "$['x",
            "$[?(@.id)]",
            "$[0:1]",
            "$['\\q']",
            "$[9007199254740992]",
        ] {
            assert!(evaluate(&json!({}), path).is_err(), "{path}");
        }
        assert!(evaluate(&json!({}), &format!("$.{}", "x".repeat(255))).is_err());
        assert!(evaluate(&json!({"items":vec![json!({"id":1}); 10_001]}), "$..id").is_err());
    }

    #[test]
    fn quoted_unicode_and_escaped_members_match_the_frontend_subset() {
        let doc = json!({"한글":1,"emoji😀":2,"a'b":3,"a\\b":4});
        assert_eq!(evaluate(&doc, "$['한글']").unwrap(), vec![&json!(1)]);
        assert_eq!(
            evaluate(&doc, "$['emoji\\ud83d\\ude00']").unwrap(),
            vec![&json!(2)]
        );
        assert_eq!(evaluate(&doc, "$['a\\'b']").unwrap(), vec![&json!(3)]);
        assert_eq!(evaluate(&doc, "$['a\\\\b']").unwrap(), vec![&json!(4)]);
    }
}
