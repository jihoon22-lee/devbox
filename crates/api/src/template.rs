//! 활성 필드의 `{{name}}`·`${name}` 변수 치환과 `{{secret:NAME}}` 참조 해석.
//!
//! 한 번만 치환한다(값 안의 변수는 다시 풀지 않음). 정의 안 된 `{{name}}`은 보내지 않고
//! `api.variable_missing`으로, 정의 안 된 `${name}`은 v0.9.0처럼 글자 그대로 둔다.
use crate::{ApiError, Auth, Body, EnvSnapshot, Part, RequestSnapshot, VarOrigin};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn variable_name(inner: &str) -> Option<&str> {
    let name = inner.trim_matches([' ', '\t']);
    (!name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')))
    .then_some(name)
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Text(&'a str),
    /// 캡처 값: 표시 모드면 자리 표시(`Literals`)로 넣는다.
    Literal(&'a str, usize),
}

pub(crate) struct Vars<'a> {
    values: HashMap<&'a str, Value<'a>>,
    /// 있으면 캡처 값을 이 nonce의 자리 표시로 넣는다(비밀 해석이 끝난 뒤 `Literals::restore`).
    nonce: Option<String>,
}

impl<'a> Vars<'a> {
    /// 같은 이름이 여러 번 오면 뒤의 값(세션이 환경을 덮음)을 쓴다. 캡처 값도 글자 그대로 넣는다.
    #[cfg(test)]
    pub(crate) fn new(env: &'a EnvSnapshot) -> Self {
        Self::build(env, None)
    }

    fn build(env: &'a EnvSnapshot, nonce: Option<String>) -> Self {
        Self {
            values: env
                .variables
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let value = match v.origin {
                        VarOrigin::Environment => Value::Text(v.value.as_str()),
                        VarOrigin::Capture => Value::Literal(v.value.as_str(), i),
                    };
                    (v.name.as_str(), value)
                })
                .collect(),
            nonce,
        }
    }

    fn push(&self, out: &mut String, value: Value<'_>) {
        match (value, &self.nonce) {
            (Value::Text(text), _) | (Value::Literal(text, _), None) => out.push_str(text),
            (Value::Literal(_, index), Some(nonce)) => {
                out.push(LITERAL_OPEN);
                out.push_str(nonce);
                out.push(':');
                out.push_str(&index.to_string());
                out.push(LITERAL_CLOSE);
            }
        }
    }

    pub(crate) fn apply(&self, text: &str, missing: &mut BTreeSet<String>) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(['{', '$']) {
            out.push_str(&rest[..at]);
            let tail = &rest[at..];
            let (open, close, braced) = if tail.starts_with("{{") {
                ("{{", "}}", true)
            } else if tail.starts_with("${") {
                ("${", "}", false)
            } else {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
                continue;
            };
            let body = &tail[open.len()..];
            match body
                .find(close)
                .and_then(|end| variable_name(&body[..end]).map(|n| (n, end)))
            {
                Some((name, end)) => {
                    match self.values.get(name) {
                        Some(value) => self.push(&mut out, *value),
                        None => {
                            if braced {
                                missing.insert(name.to_owned());
                            }
                            out.push_str(&tail[..open.len() + end + close.len()]);
                        }
                    }
                    rest = &body[end + close.len()..];
                }
                None => {
                    out.push_str(open);
                    rest = body;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

const SECRET_OPEN: &str = "{{secret:";
const LITERAL_OPEN: char = '\u{E000}';
const LITERAL_CLOSE: char = '\u{E001}';

/// `{{secret:NAME}}` 이름들. 닫히지 않았거나 이름이 틀리면 보내지 않는다.
pub(crate) fn secret_names(text: &str, out: &mut BTreeSet<String>) -> Result<(), ApiError> {
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once(SECRET_OPEN) {
        let (name, after) = tail
            .split_once("}}")
            .ok_or_else(|| ApiError::input("secret", "reference"))?;
        if !devbox_secrets::valid_name(name) {
            return Err(ApiError::input("secret", "reference"));
        }
        out.insert(name.to_owned());
        rest = after;
    }
    Ok(())
}

pub(crate) fn replace_secrets(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, tail)) = rest.split_once(SECRET_OPEN) {
        out.push_str(before);
        let (name, after) = tail.split_once("}}").unwrap_or((tail, ""));
        out.push_str(values.get(name).map(String::as_str).unwrap_or(""));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// 활성 필드의 문자열 하나하나에 `f`를 적용한다. 헤더·쿼리·쿠키 이름과 파일 경로는 바꾸지 않는다.
pub(crate) fn visit_active(
    s: &mut RequestSnapshot,
    f: &mut dyn FnMut(&mut String) -> Result<(), ApiError>,
) -> Result<(), ApiError> {
    f(&mut s.url)?;
    for row in &mut s.query {
        f(&mut row.value)?;
    }
    for row in &mut s.headers {
        f(&mut row.value)?;
    }
    for row in &mut s.cookies {
        f(&mut row.value)?;
    }
    match &mut s.body {
        Body::None | Body::Binary { .. } => {}
        Body::Raw {
            text,
            encoding: None,
            ..
        }
        | Body::Json { text } => f(text)?,
        Body::Raw {
            encoding: Some(_), ..
        } => {}
        Body::Form { fields } => {
            for row in fields {
                f(&mut row.value)?;
            }
        }
        Body::Multipart { parts } => {
            for part in parts {
                if let Part::Text { value, .. } = part {
                    f(value)?;
                }
            }
        }
        Body::Graphql {
            query, variables, ..
        } => {
            f(query)?;
            f(variables)?;
        }
    }
    match &mut s.auth {
        Auth::None => {}
        Auth::Basic { username, password } => {
            f(username)?;
            f(password)?;
        }
        Auth::Bearer { token } => f(token)?,
        Auth::ApiKey { name, value, .. } => {
            f(name)?;
            f(value)?;
        }
        Auth::Oauth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
        } => {
            f(token_url)?;
            f(client_id)?;
            f(client_secret)?;
            if let Some(scope) = scope {
                f(scope)?;
            }
        }
        Auth::Oauth2AuthCode {
            authorization_url,
            token_url,
            client_id,
            client_secret,
            scope,
        } => {
            f(authorization_url)?;
            f(token_url)?;
            f(client_id)?;
            for value in [client_secret, scope].into_iter().flatten() {
                f(value)?;
            }
        }
    }
    for path in [
        &mut s.tls.ca_path,
        &mut s.tls.client_cert_path,
        &mut s.tls.client_key_path,
    ]
    .into_iter()
    .flatten()
    {
        f(path)?;
    }
    Ok(())
}

/// 변수 치환. 없는 `{{name}}`이 있으면 이름 목록으로 거절한다. 캡처 값은 글자 그대로 넣는다.
#[cfg(test)]
pub(crate) fn substitute(
    snapshot: &RequestSnapshot,
    env: &EnvSnapshot,
) -> Result<RequestSnapshot, ApiError> {
    let (mut out, literals) = substitute_marked(snapshot, env)?;
    literals.restore(&mut out);
    Ok(out)
}

/// 같은 이름이 환경과 캡처 두 출처로 오면 거절한다. 어느 쪽이 이기든 화면 의도와 다를 수 있고,
/// 환경 쪽 전체 참조가 이기면 캡처 이름으로 비밀이 풀린다. 출처가 같은 중복은 뒤의 값이 이긴다.
pub(crate) fn reject_mixed_origins(env: &EnvSnapshot) -> Result<(), ApiError> {
    let mut seen: HashMap<&str, VarOrigin> = HashMap::new();
    for var in &env.variables {
        if seen
            .insert(var.name.as_str(), var.origin)
            .is_some_and(|before| before != var.origin)
        {
            return Err(ApiError::input("variable", "mixed_origin"));
        }
    }
    Ok(())
}

/// 캡처 값 자리 표시. 비밀 해석(`secrets_of`·`fill_secrets`)은 자리 표시 상태에서 하므로
/// 캡처 값 안의 `{{secret:` 조각은 참조가 되지 못하고, 해석 뒤 `restore`로 글자 그대로 돌아온다.
pub(crate) struct Literals<'a> {
    marker: String,
    values: Vec<(String, &'a str)>,
}

impl Literals<'_> {
    pub(crate) fn restore(&self, snapshot: &mut RequestSnapshot) {
        if self.values.is_empty() {
            return;
        }
        let _ = visit_active(snapshot, &mut |text| {
            if text.contains(&self.marker) {
                for (token, value) in &self.values {
                    if text.contains(token.as_str()) {
                        *text = text.replace(token.as_str(), value);
                    }
                }
            }
            Ok(())
        });
    }
}

pub(crate) fn substitute_marked<'a>(
    snapshot: &RequestSnapshot,
    env: &'a EnvSnapshot,
) -> Result<(RequestSnapshot, Literals<'a>), ApiError> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let vars = Vars::build(env, Some(nonce.clone()));
    let mut missing = BTreeSet::new();
    let mut out = snapshot.clone();
    visit_active(&mut out, &mut |text| {
        *text = vars.apply(text, &mut missing);
        Ok(())
    })?;
    if !missing.is_empty() {
        return Err(ApiError::VariableMissing {
            names: missing.into_iter().collect(),
        });
    }
    let values = env
        .variables
        .iter()
        .enumerate()
        .filter(|(_, v)| v.origin == VarOrigin::Capture)
        .map(|(i, v)| {
            (
                format!("{LITERAL_OPEN}{nonce}:{i}{LITERAL_CLOSE}"),
                v.value.as_str(),
            )
        })
        .collect();
    Ok((
        out,
        Literals {
            marker: format!("{LITERAL_OPEN}{nonce}:"),
            values,
        },
    ))
}

/// 치환한 snapshot의 활성 필드가 쓰는 비밀 이름.
pub(crate) fn secrets_of(snapshot: &RequestSnapshot) -> Result<Vec<String>, ApiError> {
    let mut names = BTreeSet::new();
    let mut copy = snapshot.clone();
    visit_active(&mut copy, &mut |text| secret_names(text, &mut names))?;
    Ok(names.into_iter().collect())
}

/// 해석해도 되는 비밀 이름: 원본 활성 필드에 글자 그대로 있는 참조와, 값 전체가 비밀 참조인 환경 변수.
/// 캡처 값(`VarOrigin::Capture`)은 참조로 보지 않는다.
/// 치환으로 `{{secret:` 조각을 이어 붙여 만든 참조(저장소 파일의 재검토 판정을 우회함)는 여기에 없다.
pub(crate) fn allowed_secrets(
    snapshot: &RequestSnapshot,
    env: &EnvSnapshot,
) -> Result<BTreeSet<String>, ApiError> {
    let mut names = BTreeSet::new();
    let mut copy = snapshot.clone();
    visit_active(&mut copy, &mut |text| secret_names(text, &mut names))?;
    for var in env
        .variables
        .iter()
        .filter(|v| v.origin == VarOrigin::Environment)
    {
        let value = var.value.trim();
        if is_reference(value) && value.starts_with(SECRET_OPEN) {
            secret_names(value, &mut names)?;
        }
    }
    Ok(names)
}

/// 활성 필드 글자 그대로의 `{{name}}`·`${name}` 변수 이름(치환 전 원본 기준).
pub(crate) fn variable_refs(text: &str, out: &mut BTreeSet<String>) {
    let mut rest = text;
    while let Some(at) = rest.find(['{', '$']) {
        let tail = &rest[at..];
        let (open, close) = if tail.starts_with("{{") {
            ("{{", "}}")
        } else if tail.starts_with("${") {
            ("${", "}")
        } else {
            rest = &tail[1..];
            continue;
        };
        let body = &tail[open.len()..];
        match body
            .find(close)
            .and_then(|end| variable_name(&body[..end]).map(|n| (n, end)))
        {
            Some((name, end)) => {
                out.insert(name.to_owned());
                rest = &body[end + close.len()..];
            }
            None => rest = body,
        }
    }
}

pub(crate) fn fill_secrets(snapshot: &mut RequestSnapshot, values: &BTreeMap<String, String>) {
    let _ = visit_active(snapshot, &mut |text| {
        if text.contains(SECRET_OPEN) {
            *text = replace_secrets(text, values);
        }
        Ok(())
    });
}

/// 값 전체가 참조 하나뿐인지(`{{name}}`·`${name}`·`{{secret:NAME}}`).
pub(crate) fn is_reference(text: &str) -> bool {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix(SECRET_OPEN)
        .and_then(|t| t.strip_suffix("}}"))
    {
        return devbox_secrets::valid_name(inner);
    }
    let inner = text
        .strip_prefix("{{")
        .and_then(|t| t.strip_suffix("}}"))
        .or_else(|| text.strip_prefix("${").and_then(|t| t.strip_suffix('}')));
    inner.and_then(variable_name).is_some() && !inner.is_some_and(|i| i.contains(['{', '}']))
}
