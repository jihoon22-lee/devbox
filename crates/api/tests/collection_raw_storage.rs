#![cfg(unix)]
use collection_fixture::*;
use devbox_api::*;
mod collection_fixture;
fn document(text: &str, content_type: &str, encoded: bool) -> String {
    let mut body = serde_json::json!({"kind":"raw","text":text,"contentType":content_type});
    if encoded {
        body["encoding"] = "base64".into();
    }
    serde_json::json!({"version":1,"name":"Received","x-extra":{"keep":true},"request":{"body":body}}).to_string()
}
#[tokio::test]
async fn direct_collection_write_rejects_unredacted_raw_json_form_and_base64() {
    let f = Fixture::new().await;
    let (root, project) = f.project("storage").await;
    let json = r#"{"client_secret":"fixture-private-value","password":"fixture-password"}"#;
    let encoded = data_encoding::BASE64.encode(json.as_bytes());
    for (index, text) in [
        document(json, "application/json", false),
        document(
            "safe=x&%70assword=fixture-password",
            "application/x-www-form-urlencoded",
            false,
        ),
        document(&encoded, "application/json", true),
        document(&encoded, "application/octet-stream", true),
        document(
            r#"{"password":"fixture-password","password":"[REDACTED]"}"#,
            "application/json",
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let path = format!("{index}.request.json");
        assert!(
            f.write(&root, &path, &text, CollectionExpect::Absent)
                .await
                .is_err(),
            "unsafe case {index} accepted"
        );
        assert!(!project.join(".devbox/api").join(path).exists());
    }
}
#[tokio::test]
async fn malformed_raw_structured_bodies_fail_closed_without_replacing_the_file() {
    let f = Fixture::new().await;
    let (root, project) = f.project("storage").await;
    let safe = document(r#"{"message":"ok"}"#, "application/json", false);
    let saved = f
        .write(&root, "a.request.json", &safe, CollectionExpect::Absent)
        .await
        .expect("owned storage fixture");
    for body in [
        document(
            r#"{"password":"fixture-password""#,
            "application/json",
            false,
        ),
        document(
            &data_encoding::BASE64.encode(b"{\"password\":\"fixture-password\",\xff}"),
            "application/json",
            true,
        ),
        document("!invalid!", "application/octet-stream", true),
    ] {
        assert!(
            f.write(
                &root,
                "a.request.json",
                &body,
                CollectionExpect::Digest {
                    digest: saved.digest.clone()
                }
            )
            .await
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(project.join(".devbox/api/a.request.json"))
                .expect("owned storage fixture"),
            safe
        );
    }
}
#[tokio::test]
async fn safe_raw_templates_redactions_and_binary_keep_exact_file_bytes() {
    let f = Fixture::new().await;
    let (root, _) = f.project("storage").await;
    for (index,body) in [document("{ \"password\": \"{{secret:TOKEN}}\", \"client_secret\": \"[REDACTED]\", \"message\": \"fine\" }","application/json",false),document("safe=a%20b&password=%5BREDACTED%5D&token=%7B%7Bsecret%3ATOKEN%7D%7D","application/x-www-form-urlencoded",false),document("AP+A","application/octet-stream",true),document("raw text stays exact","text/plain",false)].into_iter().enumerate() {
  let path=format!("{index}.request.json");let saved=f.write(&root,&path,&body,CollectionExpect::Absent).await.expect("owned storage fixture");let loaded=f.read(&root,&path).await.expect("owned storage fixture");assert_eq!(loaded.text,body);assert_eq!(loaded.digest,saved.digest);
 }
}

#[tokio::test]
async fn raw_malformed_json_cannot_hide_escaped_sensitive_keys_as_plain_text() {
    let f = Fixture::new().await;
    let (root, project) = f.project("storage").await;
    let text = document(
        r#"{"\u0070assword":"fixture-password""#,
        "text/plain",
        false,
    );
    assert!(
        f.write(
            &root,
            "malformed.request.json",
            &text,
            CollectionExpect::Absent
        )
        .await
        .is_err()
    );
    assert!(!project.join(".devbox/api/malformed.request.json").exists());
}

#[tokio::test]
async fn wrapped_known_tokens_are_rejected_in_plain_json_and_decoded_base64() {
    let f = Fixture::new().await;
    let (root, project) = f.project("wrapped-storage").await;
    let token = "ghp_abcdefghijklmnopqrstuvwxyz1234567890";
    for (index, (body, mime, encoded)) in [
        (format!("({token})"), "text/plain", false),
        (
            serde_json::json!({"message": format!("({token})")}).to_string(),
            "application/json",
            false,
        ),
        (
            data_encoding::BASE64.encode(format!("[{token}]").as_bytes()),
            "application/octet-stream",
            true,
        ),
        (
            data_encoding::BASE64.encode(
                serde_json::json!({"message": format!("<{token}>")})
                    .to_string()
                    .as_bytes(),
            ),
            "application/json",
            true,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let path = format!("{index}.request.json");
        assert!(
            f.write(
                &root,
                &path,
                &document(&body, mime, encoded),
                CollectionExpect::Absent
            )
            .await
            .is_err(),
            "wrapped token case {index} was accepted"
        );
        assert!(!project.join(".devbox/api").join(path).exists());
    }
}

#[derive(serde::Deserialize)]
struct TokenCase {
    label: String,
    text: String,
    sensitive: bool,
}
#[tokio::test]
async fn stored_token_golden_corpus_matches_plain_json_and_base64_writes() {
    let cases: Vec<TokenCase> =
        serde_json::from_str(include_str!("fixtures/raw-storage-token-cases.json"))
            .expect("owned storage fixture");
    let f = Fixture::new().await;
    let (root, project) = f.project("token-corpus").await;
    for (case_index, case) in cases.iter().enumerate() {
        for (mode, (body, mime, encoded)) in [
            (case.text.clone(), "text/plain", false),
            (
                serde_json::json!({"message":case.text}).to_string(),
                "application/json",
                false,
            ),
            (
                data_encoding::BASE64.encode(case.text.as_bytes()),
                "application/octet-stream",
                true,
            ),
            (
                data_encoding::BASE64.encode(
                    serde_json::json!({"message":case.text})
                        .to_string()
                        .as_bytes(),
                ),
                "application/json",
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let path = format!("case-{case_index}-{mode}.request.json");
            let text = document(&body, mime, encoded);
            let result = f.write(&root, &path, &text, CollectionExpect::Absent).await;
            assert_eq!(
                result.is_err(),
                case.sensitive,
                "{} mode {mode}",
                case.label
            );
            if case.sensitive {
                assert!(!project.join(".devbox/api").join(&path).exists());
            } else {
                assert_eq!(
                    f.read(&root, &path)
                        .await
                        .expect("owned storage fixture")
                        .text,
                    text
                );
            }
        }
    }
}
