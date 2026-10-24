//! v0.9.0 `environments.test.ts` 치환 규칙 7개 이식 + 적응 1개.
use crate::template::{Vars, secrets_of, substitute};
use crate::*;
use std::collections::BTreeSet;

fn env(pairs: &[(&str, &str)]) -> EnvSnapshot {
    EnvSnapshot {
        variables: pairs
            .iter()
            .map(|(n, v)| EnvVar {
                name: (*n).into(),
                value: (*v).into(),
                sensitive: false,
                origin: VarOrigin::Environment,
            })
            .collect(),
    }
}
fn apply(text: &str, pairs: &[(&str, &str)]) -> (String, BTreeSet<String>) {
    let env = env(pairs);
    let mut missing = BTreeSet::new();
    (Vars::new(&env).apply(text, &mut missing), missing)
}
fn snapshot() -> RequestSnapshot {
    RequestSnapshot {
        method: "POST".into(),
        url: "https://example.test".into(),
        query: vec![],
        headers: vec![],
        cookies: vec![],
        body: Body::None,
        auth: Auth::None,
        tls: TlsOptions::default(),
        timeout_ms: 10_000,
        follow_redirects: true,
    }
}
fn pair(name: &str, value: &str) -> Pair {
    Pair {
        name: name.into(),
        value: value.into(),
    }
}
fn row(row: u32, name: &str, value: &str) -> HeaderRow {
    HeaderRow {
        row,
        name: name.into(),
        value: value.into(),
    }
}

#[test]
fn basic_substitution() {
    let (out, _) = apply(
        "{{base}}/v1?token={{token}}",
        &[("base", "https://api.example.com"), ("token", "abc")],
    );
    assert_eq!(out, "https://api.example.com/v1?token=abc");
}

#[test]
fn dollar_and_braced_references_substitute_together() {
    let (out, _) = apply(
        "${BASE_URL}/${VERSION}?token={{TOKEN}}",
        &[
            ("BASE_URL", "https://api.example.com"),
            ("VERSION", "v2"),
            ("TOKEN", "abc"),
        ],
    );
    assert_eq!(out, "https://api.example.com/v2?token=abc");
}

#[test]
fn whitespace_inside_braces_is_ignored() {
    assert_eq!(apply("{{ a }}", &[("a", "x")]).0, "x");
    assert_eq!(apply("${ a }", &[("a", "x")]).0, "x");
}

#[test]
fn request_template_is_not_mutated() {
    let original = RequestSnapshot {
        url: "{{base}}/x".into(),
        body: Body::Raw {
            encoding: None,
            text: "hello {{name}}".into(),
            content_type: None,
        },
        headers: vec![HeaderRow {
            row: 0,
            name: "A".into(),
            value: "{{t}}".into(),
        }],
        ..snapshot()
    };
    let out = substitute(
        &original,
        &env(&[("base", "https://e"), ("name", "world"), ("t", "v")]),
    )
    .expect("substitute");
    assert_eq!(out.url, "https://e/x");
    assert_eq!(
        out.body,
        Body::Raw {
            encoding: None,
            text: "hello world".into(),
            content_type: None
        }
    );
    assert_eq!(out.headers[0].value, "v");
    assert_eq!(original.url, "{{base}}/x");
}

#[test]
fn url_params_headers_body_and_every_auth_field_are_substituted() {
    let original = RequestSnapshot {
        url: "${BASE_URL}/${VERSION}".into(),
        headers: vec![HeaderRow {
            row: 0,
            name: "Authorization".into(),
            value: "Bearer ${TOKEN}".into(),
        }],
        cookies: vec![row(0, "session", "${TOKEN}")],
        query: vec![pair("tenant", "${TENANT}")],
        body: Body::Multipart {
            parts: vec![
                Part::Text {
                    name: "token".into(),
                    value: "${TOKEN}".into(),
                    content_type: Some("text/plain".into()),
                },
                Part::File {
                    name: "upload".into(),
                    path: "/tmp/${TOKEN}/artifact.zip".into(),
                    file_name: Some("artifact.zip".into()),
                    content_type: Some("application/zip".into()),
                },
            ],
        },
        auth: Auth::ApiKey {
            name: "${API_HEADER}".into(),
            value: "${API_VALUE}".into(),
            placement: KeyPlacement::Header,
        },
        ..snapshot()
    };
    let vars = env(&[
        ("BASE_URL", "https://api.example.com"),
        ("VERSION", "v2"),
        ("TOKEN", "token-value"),
        ("TENANT", "tenant-value"),
        ("API_HEADER", "X-API-Key"),
        ("API_VALUE", "api-value"),
    ]);
    let out = substitute(&original, &vars).expect("substitute");
    assert_eq!(out.url, "https://api.example.com/v2");
    assert_eq!(out.headers[0].value, "Bearer token-value");
    assert_eq!(out.cookies, [row(0, "session", "token-value")]);
    assert_eq!(out.query[0].value, "tenant-value");
    let Body::Multipart { parts } = &out.body else {
        panic!("multipart")
    };
    assert!(matches!(&parts[0], Part::Text { value, .. } if value == "token-value"));
    assert!(matches!(&parts[1], Part::File { path, .. } if path == "/tmp/${TOKEN}/artifact.zip"));
    assert_eq!(
        out.auth,
        Auth::ApiKey {
            name: "X-API-Key".into(),
            value: "api-value".into(),
            placement: KeyPlacement::Header
        }
    );
    let basic = substitute(
        &RequestSnapshot {
            auth: Auth::Basic {
                username: "${USERNAME}".into(),
                password: "${PASSWORD}".into(),
            },
            ..snapshot()
        },
        &env(&[("USERNAME", "user-value"), ("PASSWORD", "password-value")]),
    )
    .expect("basic");
    assert_eq!(
        basic.auth,
        Auth::Basic {
            username: "user-value".into(),
            password: "password-value".into()
        }
    );
    assert_eq!(original.cookies[0].value, "${TOKEN}");
}

/// v0.9.0 "multipart에서는 stale body를 해석하지 않고 비운다" → snapshot에는 활성 본문 하나뿐이다.
#[test]
fn multipart_snapshot_has_no_stale_text_body_to_resolve() {
    let out = substitute(
        &RequestSnapshot {
            body: Body::Multipart { parts: vec![] },
            ..snapshot()
        },
        &env(&[("BROKEN", "must-not-be-used")]),
    )
    .expect("substitute");
    assert_eq!(out.body, Body::Multipart { parts: vec![] });
}

#[test]
fn unknown_dollar_reference_is_preserved() {
    let (out, missing) = apply("${KNOWN}/${MISSING}", &[("KNOWN", "ok")]);
    assert_eq!(out, "ok/${MISSING}");
    assert!(missing.is_empty());
}

/// 적응: v0.9.0 "알 수 없는 변수는 그대로" → v1은 송신 전 `api.variable_missing{ names }`.
#[test]
fn unknown_braced_variable_is_rejected_before_sending() {
    let (out, missing) = apply("{{a}}-{{missing}}", &[("a", "1")]);
    assert_eq!(out, "1-{{missing}}");
    assert_eq!(missing.into_iter().collect::<Vec<_>>(), ["missing"]);
    let err = substitute(
        &RequestSnapshot {
            url: "https://e/{{missing}}/{{other}}".into(),
            ..snapshot()
        },
        &env(&[]),
    )
    .expect_err("missing variables");
    assert_eq!(
        err,
        ApiError::VariableMissing {
            names: vec!["missing".into(), "other".into()]
        }
    );
}

#[test]
fn variables_resolve_both_styles_in_active_fields_only() {
    let original = RequestSnapshot {
        url: "https://e/{{a}}/${b}".into(),
        headers: vec![HeaderRow {
            row: 4,
            name: "{{name}}".into(),
            value: "{{secret:HEADER}}".into(),
        }],
        body: Body::Json {
            text: r#"{"t":"{{tok}}"}"#.into(),
        },
        ..snapshot()
    };
    let vars = env(&[("a", "1"), ("b", "2"), ("tok", "{{secret:FROM_ENV}}")]);
    let out = substitute(&original, &vars).expect("substitute");
    assert_eq!(out.url, "https://e/1/2");
    assert_eq!(out.headers[0].name, "{{name}}", "header names are literal");
    assert_eq!(
        secrets_of(&out).expect("names"),
        ["FROM_ENV".to_string(), "HEADER".into()]
    );
    // `{{secret:X}}`는 변수가 아니며, 값 안의 참조도 한 번만 풀린다.
    let (out, missing) = apply("{{secret:X}} {{v}}", &[("v", "{{w}}")]);
    assert_eq!(out, "{{secret:X}} {{w}}");
    assert!(missing.is_empty());
    for bad in ["{{secret:bad name}}", "{{secret:OPEN"] {
        let mut names = BTreeSet::new();
        assert!(
            crate::template::secret_names(bad, &mut names).is_err(),
            "{bad}"
        );
    }
}
