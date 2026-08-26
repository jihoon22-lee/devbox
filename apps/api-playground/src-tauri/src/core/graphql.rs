use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub const MAX_GRAPHQL_QUERY_BYTES: usize = 512 * 1024;
pub const MAX_GRAPHQL_VARIABLES_BYTES: usize = 512 * 1024;
pub const MAX_GRAPHQL_OPERATION_NAME_BYTES: usize = 128;
pub const MAX_GRAPHQL_OPERATIONS: usize = 100;
pub const MAX_GRAPHQL_TOKENS: usize = 100_000;
pub const MAX_GRAPHQL_VARIABLE_DEPTH: usize = 32;
pub const MAX_GRAPHQL_VARIABLE_NODES: usize = 10_000;
pub const MAX_GRAPHQL_VARIABLE_STRING_BYTES: usize = 64 * 1024;
pub const MAX_GRAPHQL_BODY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_GRAPHQL_RESPONSE_NODES: usize = 10_000;
pub const MAX_GRAPHQL_RESPONSE_DEPTH: usize = 64;
pub const MAX_GRAPHQL_RESPONSE_STRING_BYTES: usize = 64 * 1024;
pub const MAX_GRAPHQL_RESPONSE_ERRORS: usize = 100;
pub const MAX_GRAPHQL_ERROR_MESSAGE_BYTES: usize = 4 * 1024;
pub const MAX_GRAPHQL_ERROR_PATH_ITEMS: usize = 20;
pub const MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES: usize = 128;
pub const MAX_GRAPHQL_ERROR_LOCATION: u64 = 9_007_199_254_740_991;

pub const GRAPHQL_INVALID_REQUEST: &str = "GraphQL 요청 구성이 올바르지 않습니다";
pub const GRAPHQL_INVALID_DOCUMENT: &str = "GraphQL 문서 형식이 올바르지 않습니다";
pub const GRAPHQL_QUERY_TOO_LARGE: &str = "GraphQL query가 허용된 크기를 초과했습니다";
pub const GRAPHQL_VARIABLES_TOO_LARGE: &str = "GraphQL variables가 허용된 크기를 초과했습니다";
pub const GRAPHQL_OPERATION_INVALID: &str = "GraphQL operation 선택이 올바르지 않습니다";
pub const GRAPHQL_VARIABLES_INVALID: &str = "GraphQL variables는 유효한 JSON object여야 합니다";
pub const GRAPHQL_VARIABLES_TOO_COMPLEX: &str =
    "GraphQL variables 구조가 허용된 한계를 초과했습니다";
pub const GRAPHQL_BODY_TOO_LARGE: &str = "GraphQL 요청 본문이 허용된 크기를 초과했습니다";
pub const GRAPHQL_UNSUPPORTED_INTROSPECTION: &str =
    "GraphQL introspection 요청은 지원하지 않습니다";
pub const GRAPHQL_UNSUPPORTED_SUBSCRIPTION: &str = "GraphQL subscription은 지원하지 않습니다";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GraphqlRequest {
    pub query: String,
    pub variables: String,
    pub operation_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphqlLocation {
    pub line: u64,
    pub column: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphqlError {
    pub message: String,
    pub locations: Vec<GraphqlLocation>,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphqlResponse {
    /// valid | not_json | invalid | oversized
    pub envelope: String,
    pub data: Option<Value>,
    pub errors: Vec<GraphqlError>,
    pub errors_truncated: bool,
}

#[derive(Debug, Clone)]
struct DocumentInfo {
    operations: usize,
    names: Vec<String>,
}

#[derive(Debug, Clone)]
enum Token {
    Name(String),
    String,
    Number,
    Punct(char),
    Spread,
}

pub fn build_request_body(request: &GraphqlRequest) -> Result<String, String> {
    validate_document(&request.query, &request.operation_name)?;
    let variables = parse_variables(&request.variables)?;
    if request.query.trim().is_empty() {
        return Err(GRAPHQL_INVALID_DOCUMENT.to_string());
    }

    let mut body = Map::new();
    body.insert(
        "query".to_string(),
        Value::String(request.query.trim().to_string()),
    );
    body.insert("variables".to_string(), variables);
    if !request.operation_name.trim().is_empty() {
        body.insert(
            "operationName".to_string(),
            Value::String(request.operation_name.trim().to_string()),
        );
    }
    let serialized = serde_json::to_string(&Value::Object(body))
        .map_err(|_| GRAPHQL_INVALID_REQUEST.to_string())?;
    if serialized.len() > MAX_GRAPHQL_BODY_BYTES {
        return Err(GRAPHQL_BODY_TOO_LARGE.to_string());
    }
    Ok(serialized)
}

pub fn validate_document(query: &str, requested_operation_name: &str) -> Result<(), String> {
    let info = parse_document(query)?;
    let requested = requested_operation_name.trim();
    if requested.len() > MAX_GRAPHQL_OPERATION_NAME_BYTES
        || (!requested.is_empty() && !is_name(requested))
    {
        return Err(GRAPHQL_OPERATION_INVALID.to_string());
    }

    // GraphQL-over-HTTP permits selecting one named operation from a
    // multi-operation document. An anonymous operation, however, is valid
    // only when it is the document's sole operation.
    if info.operations > 1 && info.names.len() != info.operations {
        return Err(GRAPHQL_OPERATION_INVALID.to_string());
    }
    if info.operations > 1 && requested.is_empty() {
        return Err(GRAPHQL_OPERATION_INVALID.to_string());
    }
    if !requested.is_empty() && !info.names.iter().any(|name| name == requested) {
        return Err(GRAPHQL_OPERATION_INVALID.to_string());
    }
    Ok(())
}

pub fn parse_variables(raw: &str) -> Result<Value, String> {
    if raw.len() > MAX_GRAPHQL_VARIABLES_BYTES {
        return Err(GRAPHQL_VARIABLES_TOO_LARGE.to_string());
    }
    if raw.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    // Reject pathological nesting before serde_json enters its recursive
    // parser. The root object adds one structural level to the value depth
    // used by validate_json_bounds.
    if !json_depth_within(raw, MAX_GRAPHQL_VARIABLE_DEPTH.saturating_add(1)) {
        return Err(GRAPHQL_VARIABLES_TOO_COMPLEX.to_string());
    }
    let parsed =
        serde_json::from_str::<Value>(raw).map_err(|_| GRAPHQL_VARIABLES_INVALID.to_string())?;
    if !parsed.is_object() {
        return Err(GRAPHQL_VARIABLES_INVALID.to_string());
    }
    let mut nodes = 0;
    validate_json_bounds(
        &parsed,
        0,
        &mut nodes,
        MAX_GRAPHQL_VARIABLE_DEPTH,
        MAX_GRAPHQL_VARIABLE_NODES,
        MAX_GRAPHQL_VARIABLE_STRING_BYTES,
    )?;
    Ok(parsed)
}

pub fn validate_json_bounds(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    max_depth: usize,
    max_nodes: usize,
    max_string_bytes: usize,
) -> Result<(), String> {
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| GRAPHQL_VARIABLES_TOO_COMPLEX.to_string())?;
    if *nodes > max_nodes || depth > max_depth {
        return Err(GRAPHQL_VARIABLES_TOO_COMPLEX.to_string());
    }
    match value {
        Value::String(text) if text.len() > max_string_bytes => {
            Err(GRAPHQL_VARIABLES_TOO_COMPLEX.to_string())
        }
        Value::Array(items) => {
            for item in items {
                validate_json_bounds(
                    item,
                    depth + 1,
                    nodes,
                    max_depth,
                    max_nodes,
                    max_string_bytes,
                )?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for (key, child) in object {
                if key.len() > max_string_bytes {
                    return Err(GRAPHQL_VARIABLES_TOO_COMPLEX.to_string());
                }
                validate_json_bounds(
                    child,
                    depth + 1,
                    nodes,
                    max_depth,
                    max_nodes,
                    max_string_bytes,
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn parse_response(body: &str) -> GraphqlResponse {
    // Keep malformed/hostile deeply nested JSON away from serde_json's
    // recursive parser. The response envelope and `data` wrapper account for
    // two structural levels in addition to the projected data depth.
    if !json_depth_within(body, MAX_GRAPHQL_RESPONSE_DEPTH.saturating_add(2)) {
        return GraphqlResponse {
            envelope: "oversized".to_string(),
            data: None,
            errors: Vec::new(),
            errors_truncated: false,
        };
    }
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return GraphqlResponse {
            envelope: "not_json".to_string(),
            data: None,
            errors: Vec::new(),
            errors_truncated: false,
        };
    };
    let Some(object) = value.as_object() else {
        return invalid_response();
    };
    let has_data = object.contains_key("data");
    let has_errors = object.contains_key("errors");
    if !has_data && !has_errors {
        return invalid_response();
    }

    let data = if let Some(data) = object.get("data") {
        let mut nodes = 0;
        if validate_json_bounds(
            data,
            0,
            &mut nodes,
            MAX_GRAPHQL_RESPONSE_DEPTH,
            MAX_GRAPHQL_RESPONSE_NODES,
            MAX_GRAPHQL_RESPONSE_STRING_BYTES,
        )
        .is_err()
        {
            return GraphqlResponse {
                envelope: "oversized".to_string(),
                data: None,
                errors: Vec::new(),
                errors_truncated: false,
            };
        }
        Some(data.clone())
    } else {
        None
    };

    let (errors, errors_truncated) = if let Some(raw_errors) = object.get("errors") {
        let Some(items) = raw_errors.as_array() else {
            return invalid_response();
        };
        let truncated = items.len() > MAX_GRAPHQL_RESPONSE_ERRORS;
        let mut projected = Vec::new();
        for item in items.iter().take(MAX_GRAPHQL_RESPONSE_ERRORS) {
            let Some(item) = item.as_object() else {
                return invalid_response();
            };
            let Some(message) = item.get("message").and_then(Value::as_str) else {
                return invalid_response();
            };
            if message.len() > MAX_GRAPHQL_ERROR_MESSAGE_BYTES {
                return GraphqlResponse {
                    envelope: "oversized".to_string(),
                    data: None,
                    errors: Vec::new(),
                    errors_truncated: false,
                };
            }
            let locations = item
                .get("locations")
                .and_then(Value::as_array)
                .map(|locations| {
                    locations
                        .iter()
                        .take(20)
                        .filter_map(|location| {
                            let object = location.as_object()?;
                            let line = object.get("line")?.as_u64()?;
                            let column = object.get("column")?.as_u64()?;
                            if line > MAX_GRAPHQL_ERROR_LOCATION
                                || column > MAX_GRAPHQL_ERROR_LOCATION
                            {
                                return None;
                            }
                            Some(GraphqlLocation { line, column })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let path = item
                .get("path")
                .and_then(Value::as_array)
                .map(|path| {
                    path.iter()
                        .take(MAX_GRAPHQL_ERROR_PATH_ITEMS)
                        .filter_map(|item| match item {
                            Value::String(text)
                                if text.len() <= MAX_GRAPHQL_ERROR_PATH_ITEM_BYTES =>
                            {
                                Some(text.clone())
                            }
                            Value::Number(number) => number.as_u64().map(|value| value.to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            projected.push(GraphqlError {
                message: message.to_string(),
                locations,
                path,
            });
        }
        (projected, truncated)
    } else {
        (Vec::new(), false)
    };

    GraphqlResponse {
        envelope: "valid".to_string(),
        data,
        errors,
        errors_truncated,
    }
}

fn json_depth_within(raw: &str, max_depth: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in raw.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = match depth.checked_add(1) {
                    Some(value) => value,
                    None => return false,
                };
                if depth > max_depth {
                    return false;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}

fn invalid_response() -> GraphqlResponse {
    GraphqlResponse {
        envelope: "invalid".to_string(),
        data: None,
        errors: Vec::new(),
        errors_truncated: false,
    }
}

fn parse_document(query: &str) -> Result<DocumentInfo, String> {
    if query.len() > MAX_GRAPHQL_QUERY_BYTES {
        return Err(GRAPHQL_QUERY_TOO_LARGE.to_string());
    }
    let tokens = lex(query)?;
    let mut index = 0;
    let mut operations = 0;
    let mut names = Vec::new();
    let mut name_set = BTreeSet::new();

    while index < tokens.len() {
        match &tokens[index] {
            Token::Name(kind) if matches!(kind.as_str(), "query" | "mutation" | "subscription") => {
                if kind == "subscription" {
                    return Err(GRAPHQL_UNSUPPORTED_SUBSCRIPTION.to_string());
                }
                operations += 1;
                if operations > MAX_GRAPHQL_OPERATIONS {
                    return Err(GRAPHQL_OPERATION_INVALID.to_string());
                }
                index += 1;
                let operation_name = if let Some(Token::Name(name)) = tokens.get(index) {
                    index += 1;
                    Some(name.clone())
                } else {
                    None
                };
                if let Some(name) = operation_name {
                    if name.len() > MAX_GRAPHQL_OPERATION_NAME_BYTES
                        || !name_set.insert(name.clone())
                    {
                        return Err(GRAPHQL_OPERATION_INVALID.to_string());
                    }
                    names.push(name);
                }
                skip_to_selection(&tokens, &mut index)?;
            }
            Token::Name(kind) if kind == "fragment" => {
                index += 1;
                expect_name(&tokens, &mut index)?;
                match tokens.get(index) {
                    Some(Token::Name(on)) if on == "on" => index += 1,
                    _ => return Err(GRAPHQL_INVALID_DOCUMENT.to_string()),
                }
                expect_name(&tokens, &mut index)?;
                skip_to_selection(&tokens, &mut index)?;
            }
            Token::Punct('{') => {
                operations += 1;
                if operations > MAX_GRAPHQL_OPERATIONS {
                    return Err(GRAPHQL_OPERATION_INVALID.to_string());
                }
                skip_group(&tokens, &mut index)?;
            }
            _ => return Err(GRAPHQL_INVALID_DOCUMENT.to_string()),
        }
    }
    if operations == 0 {
        return Err(GRAPHQL_INVALID_DOCUMENT.to_string());
    }
    Ok(DocumentInfo { operations, names })
}

fn lex(query: &str) -> Result<Vec<Token>, String> {
    let bytes = query.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' | b',' => index += 1,
            b'#' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'_' | b'A'..=b'Z' | b'a'..=b'z' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let name = query[start..index].to_string();
                if name == "__schema" || name == "__type" {
                    return Err(GRAPHQL_UNSUPPORTED_INTROSPECTION.to_string());
                }
                tokens.push(Token::Name(name));
            }
            b'"' => {
                let (next, _block) = scan_string(bytes, index)?;
                tokens.push(Token::String);
                index = next;
            }
            b'.' if bytes.get(index..index + 3) == Some(b"...") => {
                tokens.push(Token::Spread);
                index += 3;
            }
            b'!' | b'$' | b'&' | b'(' | b')' | b':' | b'=' | b'@' | b'[' | b']' | b'{' | b'|'
            | b'}' => {
                tokens.push(Token::Punct(bytes[index] as char));
                index += 1;
            }
            b'-' | b'0'..=b'9' => {
                index += 1;
                while index < bytes.len()
                    && !matches!(
                        bytes[index],
                        b' ' | b'\t'
                            | b'\r'
                            | b'\n'
                            | b','
                            | b'('
                            | b')'
                            | b'['
                            | b']'
                            | b'{'
                            | b'}'
                    )
                {
                    index += 1;
                }
                tokens.push(Token::Number);
            }
            _ => return Err(GRAPHQL_INVALID_DOCUMENT.to_string()),
        }
        if tokens.len() > MAX_GRAPHQL_TOKENS {
            return Err(GRAPHQL_QUERY_TOO_LARGE.to_string());
        }
    }
    Ok(tokens)
}

fn scan_string(bytes: &[u8], start: usize) -> Result<(usize, bool), String> {
    let block = bytes.get(start..start + 3) == Some(b"\"\"\"");
    let mut index = start + if block { 3 } else { 1 };
    while index < bytes.len() {
        if block && bytes.get(index..index + 3) == Some(b"\"\"\"") {
            return Ok((index + 3, true));
        }
        if !block && bytes[index] == b'"' {
            return Ok((index + 1, false));
        }
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
            continue;
        }
        if !block && (bytes[index] < 0x20 || bytes[index] == 0x7f) {
            return Err(GRAPHQL_INVALID_DOCUMENT.to_string());
        }
        index += 1;
    }
    Err(GRAPHQL_INVALID_DOCUMENT.to_string())
}

fn skip_to_selection(tokens: &[Token], index: &mut usize) -> Result<(), String> {
    let mut paren = 0;
    let mut bracket = 0;
    while *index < tokens.len() {
        match tokens[*index] {
            Token::Punct('(') => paren += 1,
            Token::Punct(')') if paren > 0 => paren -= 1,
            Token::Punct('[') => bracket += 1,
            Token::Punct(']') if bracket > 0 => bracket -= 1,
            Token::Punct(')') | Token::Punct(']') | Token::Punct('}') => {
                return Err(GRAPHQL_INVALID_DOCUMENT.to_string())
            }
            Token::Punct('{') if paren == 0 && bracket == 0 => return skip_group(tokens, index),
            _ => {}
        }
        *index += 1;
    }
    Err(GRAPHQL_INVALID_DOCUMENT.to_string())
}

fn skip_group(tokens: &[Token], index: &mut usize) -> Result<(), String> {
    let Some(Token::Punct(open)) = tokens.get(*index) else {
        return Err(GRAPHQL_INVALID_DOCUMENT.to_string());
    };
    let close = match open {
        '{' => '}',
        '[' => ']',
        '(' => ')',
        _ => return Err(GRAPHQL_INVALID_DOCUMENT.to_string()),
    };
    let mut stack = vec![close];
    *index += 1;
    while let Some(token) = tokens.get(*index) {
        match token {
            Token::Punct('{') => stack.push('}'),
            Token::Punct('[') => stack.push(']'),
            Token::Punct('(') => stack.push(')'),
            Token::Punct(close) if stack.last() == Some(close) => {
                stack.pop();
                if stack.is_empty() {
                    *index += 1;
                    return Ok(());
                }
            }
            Token::Punct(')') | Token::Punct(']') | Token::Punct('}') => {
                return Err(GRAPHQL_INVALID_DOCUMENT.to_string())
            }
            _ => {}
        }
        *index += 1;
    }
    Err(GRAPHQL_INVALID_DOCUMENT.to_string())
}

fn expect_name(tokens: &[Token], index: &mut usize) -> Result<(), String> {
    match tokens.get(*index) {
        Some(Token::Name(name)) if is_name(name) => {
            *index += 1;
            Ok(())
        }
        _ => Err(GRAPHQL_INVALID_DOCUMENT.to_string()),
    }
}

fn is_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('_') | Some('A'..='Z') | Some('a'..='z'))
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(query: &str, variables: &str, operation_name: &str) -> GraphqlRequest {
        GraphqlRequest {
            query: query.to_string(),
            variables: variables.to_string(),
            operation_name: operation_name.to_string(),
        }
    }

    #[test]
    fn builds_deterministic_standard_json_body_and_selects_operation() {
        let body = build_request_body(&request(
            "query GetUser($id: ID!) { user(id: $id) { id name } }",
            r#"{"id":"42"}"#,
            "GetUser",
        ))
        .unwrap();
        assert_eq!(
            body,
            r#"{"operationName":"GetUser","query":"query GetUser($id: ID!) { user(id: $id) { id name } }","variables":{"id":"42"}}"#
        );
    }

    #[test]
    fn rejects_ambiguous_operation_and_unsupported_features() {
        assert_eq!(
            build_request_body(&request("query A { a } query B { b }", "", "")).unwrap_err(),
            GRAPHQL_OPERATION_INVALID
        );
        assert_eq!(
            build_request_body(&request("subscription Events { events }", "", "Events"))
                .unwrap_err(),
            GRAPHQL_UNSUPPORTED_SUBSCRIPTION
        );
        assert_eq!(
            build_request_body(&request("{ __schema { queryType { name } } }", "", ""))
                .unwrap_err(),
            GRAPHQL_UNSUPPORTED_INTROSPECTION
        );
    }

    #[test]
    fn selects_a_named_operation_from_a_multi_operation_document() {
        let query = "query A { a } query B { b }";
        let body = build_request_body(&request(query, "", "B")).unwrap();
        assert!(body.contains("\"operationName\":\"B\""));
        assert_eq!(
            build_request_body(&request("query A { a } { b }", "", "A")).unwrap_err(),
            GRAPHQL_OPERATION_INVALID
        );
    }

    #[test]
    fn variables_are_strict_object_with_depth_node_and_string_bounds() {
        assert_eq!(
            parse_variables("[]").unwrap_err(),
            GRAPHQL_VARIABLES_INVALID
        );
        assert_eq!(
            parse_variables(&format!(
                r#"{{"value":"{}"}}"#,
                "x".repeat(MAX_GRAPHQL_VARIABLE_STRING_BYTES + 1)
            ))
            .unwrap_err(),
            GRAPHQL_VARIABLES_TOO_COMPLEX
        );
        let mut nested = "0".to_string();
        for _ in 0..=MAX_GRAPHQL_VARIABLE_DEPTH {
            nested = format!(r#"{{"next":{nested}}}"#);
        }
        assert_eq!(
            parse_variables(&nested).unwrap_err(),
            GRAPHQL_VARIABLES_TOO_COMPLEX
        );
        assert_eq!(
            build_request_body(&request(
                "{ viewer { id } }",
                r#"{"z":1,"a":{"d":2,"c":3}}"#,
                "",
            ))
            .unwrap(),
            r#"{"query":"{ viewer { id } }","variables":{"a":{"c":3,"d":2},"z":1}}"#
        );
    }

    #[test]
    fn response_projection_keeps_data_and_safe_error_fields() {
        let response = parse_response(
            r#"{"data":{"user":{"id":"1"}},"errors":[{"message":"bad","locations":[{"line":2,"column":3}],"path":["user",0],"extensions":{"token":"secret"}}]}"#,
        );
        assert_eq!(response.envelope, "valid");
        assert_eq!(response.data.as_ref().unwrap()["user"]["id"], "1");
        assert_eq!(response.errors[0].message, "bad");
        assert_eq!(response.errors[0].path, vec!["user", "0"]);
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains("extensions"));
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn malformed_response_is_fixed_projection() {
        assert_eq!(parse_response("not-json").envelope, "not_json");
        assert_eq!(parse_response("[]").envelope, "invalid");
        assert_eq!(
            parse_response(r#"{"errors":[{"message":7}]}"#).envelope,
            "invalid"
        );
        assert_eq!(
            parse_response(&format!(
                r#"{{"errors":[{{"message":"{}"}}]}}"#,
                "x".repeat(MAX_GRAPHQL_ERROR_MESSAGE_BYTES + 1)
            ))
            .envelope,
            "oversized"
        );
        assert_eq!(
            parse_response(r#"{"errors":[{"message":"bad","path":[-1,1.5]}]}"#).errors[0].path,
            Vec::<String>::new()
        );
    }

    #[test]
    fn pathological_json_nesting_is_rejected_before_recursive_parse() {
        let mut variables = String::from("{");
        for _ in 0..(MAX_GRAPHQL_VARIABLE_DEPTH + 2) {
            variables.push_str(r#"{"next":"#);
        }
        variables.push('0');
        for _ in 0..(MAX_GRAPHQL_VARIABLE_DEPTH + 2) {
            variables.push('}');
        }
        assert_eq!(
            parse_variables(&variables).unwrap_err(),
            GRAPHQL_VARIABLES_TOO_COMPLEX
        );

        let mut response = String::from(r#"{"data":"#);
        for _ in 0..(MAX_GRAPHQL_RESPONSE_DEPTH + 3) {
            response.push('{');
        }
        response.push('0');
        for _ in 0..(MAX_GRAPHQL_RESPONSE_DEPTH + 3) {
            response.push('}');
        }
        assert_eq!(parse_response(&response).envelope, "oversized");
    }
}
