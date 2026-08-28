//! Developer Toolbox's `api-request/v1` producer contract.
//!
//! The producer publishes only the explicit output currently visible in the
//! renderer.  It does not infer a URL, read a clipboard, or persist a tool's
//! input/history.  The shared applink store performs the final privacy and
//! storage validation before an envelope is written.

use serde::{Deserialize, Serialize};

pub const API_REQUEST_HANDOFF_KIND: &str = "api-request/v1";
pub const PRODUCER_APP_ID: &str = "developer-toolbox";
pub const CONSUMER_APP_ID: &str = "api-playground";
pub const HANDOFF_INPUT_ERROR: &str = "API Playground로 전달할 텍스트가 유효하지 않습니다";

/// Keep the producer boundary aligned with API Playground's request-body
/// limits.  These are deliberately smaller than the shared envelope limit so
/// one visible output cannot crowd out the envelope metadata.
pub const MAX_OUTPUT_CHARS: usize = 256_000;
pub const MAX_OUTPUT_BYTES: usize = 1_024_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiRequestHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiRequestPayload {
    pub method: String,
    pub url: String,
    pub headers: Vec<ApiRequestHeader>,
    pub body: String,
}

/// Build a bounded origin-form POST draft from the explicit output value.
///
/// No output is written here.  The command layer passes the serialized value
/// to `HandoffStore`, whose shared validator rejects raw credentials and
/// unsafe path fields before publication.
pub fn build_api_request_payload(output: &str) -> Result<ApiRequestPayload, &'static str> {
    if output.is_empty()
        || output.contains('\0')
        || output.len() > MAX_OUTPUT_BYTES
        || output.chars().count() > MAX_OUTPUT_CHARS
    {
        return Err(HANDOFF_INPUT_ERROR);
    }

    Ok(ApiRequestPayload {
        method: "POST".to_string(),
        url: "/".to_string(),
        headers: vec![ApiRequestHeader {
            name: "Content-Type".to_string(),
            value: "text/plain; charset=utf-8".to_string(),
        }],
        body: output.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::{CreateHandoff, HandoffError, HandoffStore};
    use tempfile::tempdir;

    #[test]
    fn builds_an_origin_form_post_from_explicit_output() {
        let payload = build_api_request_payload("  result\ntext  ").unwrap();

        assert_eq!(payload.method, "POST");
        assert_eq!(payload.url, "/");
        assert_eq!(payload.body, "  result\ntext  ");
        assert_eq!(
            payload.headers,
            vec![ApiRequestHeader {
                name: "Content-Type".into(),
                value: "text/plain; charset=utf-8".into(),
            }]
        );
    }

    #[test]
    fn rejects_empty_nul_and_oversized_output_before_publication() {
        assert_eq!(build_api_request_payload(""), Err(HANDOFF_INPUT_ERROR));
        assert_eq!(
            build_api_request_payload("unsafe\0output"),
            Err(HANDOFF_INPUT_ERROR)
        );
        assert_eq!(
            build_api_request_payload(&"x".repeat(MAX_OUTPUT_CHARS + 1)),
            Err(HANDOFF_INPUT_ERROR)
        );
        assert_eq!(
            build_api_request_payload(&"😀".repeat(MAX_OUTPUT_BYTES / 4 + 1)),
            Err(HANDOFF_INPUT_ERROR)
        );
    }

    #[test]
    fn payload_is_one_time_and_raw_credentials_are_not_persisted() {
        let directory = tempdir().unwrap();
        let store = HandoffStore::new(directory.path().join("handoff/v1"));
        let payload = build_api_request_payload("safe output").unwrap();
        let descriptor = store
            .create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: PRODUCER_APP_ID.into(),
                    target_app: Some(CONSUMER_APP_ID.into()),
                    payload: serde_json::to_value(payload).unwrap(),
                },
                1_000,
            )
            .unwrap();
        let claim = store
            .claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                CONSUMER_APP_ID,
                1_001,
            )
            .unwrap();
        store.ack(&claim, CONSUMER_APP_ID, 1_002).unwrap();
        assert_eq!(
            store.claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                CONSUMER_APP_ID,
                1_003,
            ),
            Err(HandoffError::Missing)
        );

        let revoked = store
            .create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: PRODUCER_APP_ID.into(),
                    target_app: Some(CONSUMER_APP_ID.into()),
                    payload: serde_json::to_value(build_api_request_payload("discard me").unwrap())
                        .unwrap(),
                },
                1_500,
            )
            .unwrap();
        store.revoke_pending(&revoked, PRODUCER_APP_ID).unwrap();
        assert_eq!(
            store.claim(
                &revoked.id,
                API_REQUEST_HANDOFF_KIND,
                CONSUMER_APP_ID,
                1_501,
            ),
            Err(HandoffError::Missing)
        );

        let raw_payload = build_api_request_payload("Bearer raw-secret").unwrap();
        assert_eq!(
            store.create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: PRODUCER_APP_ID.into(),
                    target_app: Some(CONSUMER_APP_ID.into()),
                    payload: serde_json::to_value(raw_payload).unwrap(),
                },
                2_000,
            ),
            Err(HandoffError::InvalidPayload)
        );
    }
}
