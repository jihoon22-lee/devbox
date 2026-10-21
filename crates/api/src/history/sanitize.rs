//! 기록 정화: 치환 전 snapshot에서 참조(`{{name}}`·`${name}`·`{{secret:NAME}}`)는 남기고
//! 직접 입력한 자격 증명은 `[REDACTED]`, 파일 경로는 지운다. 마지막에 이 요청의 가림기를 모든 문자열에 쓴다.
use crate::redact::Redactor;
use crate::template::is_reference;
use crate::validate::is_sensitive_name;
use crate::{Auth, Body, Part, REDACTED, RequestSnapshot};

fn literal(value: &str) -> String {
    if value.is_empty() || is_reference(value) {
        value.to_owned()
    } else {
        REDACTED.into()
    }
}

/// `Authorization: Bearer {{token}}`처럼 scheme 다음이 참조 하나면 남긴다.
fn credential_header(value: &str) -> String {
    let trimmed = value.trim();
    let rest = match trimmed.split_once(' ') {
        Some((scheme, rest)) if scheme.bytes().all(|b| b.is_ascii_alphabetic()) => rest,
        _ => trimmed,
    };
    if !rest.is_empty() && is_reference(rest) {
        trimmed.to_owned()
    } else {
        REDACTED.into()
    }
}

/// userinfo를 지우고 민감한 이름의 query 값을 가린다(템플릿이라 파싱하지 않고 문자열로 다룬다).
pub(crate) fn url(text: &str) -> String {
    let (head, fragment) = text
        .split_once('#')
        .map_or((text, None), |(h, f)| (h, Some(f)));
    let (base, query) = head
        .split_once('?')
        .map_or((head, None), |(b, q)| (b, Some(q)));
    let mut out = match base.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            format!("{scheme}://{host}{path}")
        }
        None => base.to_owned(),
    };
    if let Some(query) = query {
        let masked: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((name, value)) => {
                    let decoded: String =
                        url::form_urlencoded::parse(format!("v={value}").as_bytes())
                            .next()
                            .map(|(_, v)| v.into_owned())
                            .unwrap_or_default();
                    let name_text: String =
                        url::form_urlencoded::parse(format!("{name}=").as_bytes())
                            .next()
                            .map(|(n, _)| n.into_owned())
                            .unwrap_or_default();
                    if is_sensitive_name(&name_text)
                        && !is_reference(&decoded)
                        && !decoded.is_empty()
                    {
                        format!("{name}={REDACTED}")
                    } else {
                        pair.to_owned()
                    }
                }
                None => pair.to_owned(),
            })
            .collect();
        out.push('?');
        out.push_str(&masked.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn json_value(value: &mut serde_json::Value, sensitive: bool) {
    match value {
        serde_json::Value::String(s) if sensitive && !is_reference(s) && !s.is_empty() => {
            *s = REDACTED.into();
        }
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) if sensitive => {
            *value = serde_json::Value::String(REDACTED.into());
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| json_value(v, sensitive)),
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                json_value(v, sensitive || is_sensitive_name(key));
            }
        }
        _ => {}
    }
}

/// JSON이면 민감한 키의 직접 값을 가리고, 해석되지 않으면 민감한 키 뒤의 문자열 literal을 가린다.
pub(crate) fn json(text: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
        json_value(&mut value, false);
        return serde_json::to_string(&value).unwrap_or_else(|_| REDACTED.into());
    }
    literals(text, |name, content| {
        is_sensitive_name(name) && !is_reference(content)
    })
}

/// 문자열 literal을 훑어 `mask(앞 이름, 내용)`이 참이면 가린다. 닫히지 않은 literal은 나머지를 버린다.
fn literals(text: &str, mask: impl Fn(&str, &str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_name = String::new();
    let mut word = String::new();
    let mut key: Option<String> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '"' {
            let block = text[i..].starts_with("\"\"\"");
            let open = if block { 3 } else { 1 };
            let body_start = i + open;
            let end = if block {
                text[body_start..]
                    .find("\"\"\"")
                    .map(|e| (body_start + e, 3))
            } else {
                let mut escaped = false;
                text[body_start..]
                    .char_indices()
                    .find(|(_, ch)| {
                        let done = !escaped && *ch == '"';
                        escaped = !escaped && *ch == '\\';
                        done
                    })
                    .map(|(e, _)| (body_start + e, 1))
            };
            let Some((end, close)) = end else {
                out.push('"');
                out.push_str(REDACTED);
                return out;
            };
            let content = &text[body_start..end];
            out.push_str(&text[i..body_start]);
            out.push_str(if mask(&last_name, content) {
                REDACTED
            } else {
                content
            });
            out.push_str(&text[end..end + close]);
            last_name.clear();
            word.clear();
            key = Some(content.to_owned());
            while chars.peek().is_some_and(|(j, _)| *j < end + close) {
                chars.next();
            }
            continue;
        }
        out.push(c);
        if c.is_alphanumeric() || c == '_' || c == '-' {
            word.push(c);
            key = None;
        } else if c == ':' {
            last_name = if word.is_empty() {
                key.take().unwrap_or_default()
            } else {
                std::mem::take(&mut word)
            };
        } else if !c.is_whitespace() {
            word.clear();
            key = None;
            last_name.clear();
        }
    }
    out
}

/// `name=value` 꼴(form 본문·로그 문장)에서 민감한 이름의 직접 값을 가린다.
fn key_values(text: &str) -> String {
    let stop = |c: char| c.is_whitespace() || matches!(c, '&' | ';' | ',');
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(eq) = rest.find('=') {
        let (before, after) = (&rest[..eq], &rest[eq + 1..]);
        let name_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| name_char(*c))
            .last()
            .map_or(before.len(), |(i, _)| i);
        let value_end = after.find(stop).unwrap_or(after.len());
        let value = &after[..value_end];
        out.push_str(before);
        out.push('=');
        if is_sensitive_name(&before[name_start..]) && !value.is_empty() && !is_reference(value) {
            out.push_str(REDACTED);
        } else {
            out.push_str(value);
        }
        rest = &after[value_end..];
    }
    out.push_str(rest);
    out
}

fn raw(text: &str) -> String {
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        json(text)
    } else {
        key_values(&json(text))
    }
}

fn graphql_query(query: &str) -> String {
    literals(query, |name, content| {
        is_sensitive_name(name) && !is_reference(content)
    })
}

fn graphql_variables(text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_owned();
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(mut value) => {
            json_value(&mut value, false);
            serde_json::to_string(&value).unwrap_or_else(|_| REDACTED.into())
        }
        Err(_) => REDACTED.into(),
    }
}

fn file_name(name: &Option<String>, path: &str) -> Option<String> {
    let source = name.as_deref().filter(|n| !n.is_empty()).unwrap_or(path);
    source
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
}

pub(crate) fn snapshot(original: &RequestSnapshot, redactor: &Redactor) -> RequestSnapshot {
    let mut s = original.clone();
    s.url = url(&s.url);
    for row in &mut s.query {
        if is_sensitive_name(&row.name) {
            row.value = literal(&row.value);
        }
    }
    for row in &mut s.headers {
        let lower = row.name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "authorization" | "proxy-authorization" | "cookie"
        ) {
            row.value = credential_header(&row.value);
        } else if is_sensitive_name(&row.name) {
            row.value = literal(&row.value);
        }
    }
    for row in &mut s.cookies {
        row.value = literal(&row.value);
    }
    match &mut s.body {
        Body::None => {}
        Body::Raw { text, .. } => *text = raw(text),
        Body::Json { text } => *text = json(text),
        Body::Form { fields } => {
            for row in fields {
                if is_sensitive_name(&row.name) {
                    row.value = literal(&row.value);
                }
            }
        }
        Body::Multipart { parts } => {
            for part in parts {
                match part {
                    Part::Text { name, value, .. } => {
                        if is_sensitive_name(name) {
                            *value = literal(value);
                        }
                    }
                    Part::File {
                        path,
                        file_name: shown,
                        ..
                    } => {
                        *shown = file_name(shown, path);
                        path.clear();
                    }
                }
            }
        }
        Body::Binary { path } => path.clear(),
        Body::Graphql {
            query, variables, ..
        } => {
            *query = graphql_query(query);
            *variables = graphql_variables(variables);
        }
    }
    match &mut s.auth {
        Auth::None => {}
        Auth::Basic { password, .. } => *password = literal(password),
        Auth::Bearer { token } => *token = literal(token),
        Auth::ApiKey { value, .. } => *value = literal(value),
        Auth::Oauth2ClientCredentials {
            token_url,
            client_secret,
            ..
        } => {
            *token_url = url(token_url);
            *client_secret = literal(client_secret);
        }
        Auth::Oauth2AuthCode {
            authorization_url,
            token_url,
            client_secret,
            ..
        } => {
            *authorization_url = url(authorization_url);
            *token_url = url(token_url);
            if let Some(secret) = client_secret {
                *secret = literal(secret);
            }
        }
    }
    redact_all(&mut s, redactor);
    s
}

/// 모든 문자열(참조 포함)에 이 요청의 가림기를 쓴다.
fn redact_all(s: &mut RequestSnapshot, r: &Redactor) {
    let _ = crate::template::visit_active(s, &mut |text| {
        *text = r.text(text);
        Ok(())
    });
    for row in &mut s.query {
        row.name = r.text(&row.name);
    }
    for row in &mut s.headers {
        row.name = r.text(&row.name);
    }
    if let Body::Multipart { parts } = &mut s.body {
        for part in parts {
            if let Part::File {
                file_name: Some(n), ..
            } = part
            {
                *n = r.text(n);
            }
        }
    }
}
