//! Life Log's explicit native handoff action.
//!
//! A handoff is published only after the native digest has been rebuilt and
//! validated.  The payload is kept in the versioned one-time store; argv gets
//! only the opaque descriptor and the Knowledge executable is launched through
//! the shared launcher contract.

#[cfg(target_os = "windows")]
use crate::commands::digest::build_for_state;
use crate::commands::tracking::AppState;
use crate::core::digest::DigestInput;
#[cfg(target_os = "windows")]
use crate::core::handoff::{self, KNOWLEDGE_DRAFT_KIND};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendKnowledgeDraftResult {
    pub id: String,
    pub kind: String,
    pub expires_at_ms: u64,
}

/// Build and send a Life Log digest to Knowledge.  Browser preview and
/// non-Windows builds never publish a pending handoff or attempt a launch.
#[tauri::command]
pub async fn send_digest_to_knowledge(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
) -> Result<SendKnowledgeDraftResult, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (state, input);
        Err("Knowledge handoff는 Windows 데스크톱에서 사용할 수 없습니다".into())
    }

    #[cfg(target_os = "windows")]
    {
        // Resolve first so a missing installation does not leave a pending
        // payload behind.  A launch race can still leave an expiring pending
        // item, which contains only the bounded summary and is retryable.
        if !devbox_launch::installed_targets("handoff:knowledge-draft/v1")
            .iter()
            .any(|target| target.id == "knowledge-base")
        {
            return Err("Knowledge 앱을 실행할 수 없습니다".into());
        }
        let response = build_for_state(&state, input).await?;
        let payload = handoff::build_knowledge_draft(&response)?;
        let payload = serde_json::to_value(payload)
            .map_err(|_| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        let now_ms = current_epoch_ms()
            .ok_or_else(|| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        let store = devbox_applink::HandoffStore::new(devbox_applink::handoff_root_in(
            &devbox_integration::common_root(),
        ));
        let descriptor = store
            .create(
                devbox_applink::CreateHandoff {
                    kind: KNOWLEDGE_DRAFT_KIND.into(),
                    source_app: "life-log".into(),
                    target_app: Some("knowledge-base".into()),
                    payload,
                },
                now_ms,
            )
            .map_err(|_| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        let request = devbox_applink::OpenRequest {
            target: descriptor.clone().into(),
            from: Some("life-log".into()),
        };
        if devbox_launch::launch_open("knowledge-base", &request).is_err() {
            return Err("Knowledge 앱을 실행할 수 없습니다".into());
        }
        Ok(SendKnowledgeDraftResult {
            id: descriptor.id,
            kind: descriptor.kind,
            expires_at_ms: now_ms.saturating_add(devbox_applink::DEFAULT_HANDOFF_TTL_MS),
        })
    }
}

#[cfg(target_os = "windows")]
fn current_epoch_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|now| *now > 0)
}
