//! Bounded cross-product wire state. Native adapters must verify the peer's actual
//! process/image/installation before constructing a guard; JSON is not that proof.
use crate::{commands, installation::PRODUCTS, ProjectContext};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 512 * 1024;
pub const MAX_PENDING: usize = 1024;
pub const MAX_DEADLINE_MS: u64 = 30_000;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Hello {
    pub protocol_version: u32,
    pub product: String,
    pub installation_key: String,
    pub generation: String,
    pub session_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Commands,
    Projects,
    Repositories,
    Tasks,
    Services,
    Runs,
    Files,
    Notes,
    SavedQueries,
    Operations,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QueryMode {
    #[default]
    Name,
    Content,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Call {
    Query {
        query_id: String,
        #[serde(default)]
        mode: QueryMode,
        source: Source,
        query: String,
        generation: u64,
        context: Option<ProjectContext>,
    },
    CancelQuery {
        query_id: String,
    },
    PreviewCommand {
        request: commands::Request,
    },
    OpenCommand {
        request: commands::Request,
    },
    CommandStatus {
        operation_id: String,
    },
    Describe {},
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub installation_key: String,
    pub generation: String,
    pub session_id: String,
    pub request_id: String,
    pub deadline_ms: u64,
    pub call: Call,
}

/// Created from native observations, deliberately not Deserialize.
pub struct Peer {
    product: String,
    installation_key: String,
    generation: String,
}
impl Peer {
    /// Native process and immutable package handles must outlive this value.
    pub fn from_native(product: &str, installation_key: &str, generation: &str) -> Result<Self> {
        if !PRODUCTS.contains(&product)
            || !commands::revision(installation_key)
            || !commands::revision(generation)
        {
            return Err("peer_identity_invalid");
        }
        Ok(Self {
            product: product.into(),
            installation_key: installation_key.into(),
            generation: generation.into(),
        })
    }
}
pub struct Guard {
    peer: Peer,
    session: String,
    seen: BTreeMap<String, u64>,
}
impl Guard {
    pub fn new(peer: Peer, session: &str) -> Result<Self> {
        if !commands::opaque_id(session) {
            return Err("peer_session_invalid");
        }
        Ok(Self {
            peer,
            session: session.into(),
            seen: BTreeMap::new(),
        })
    }
    pub fn authorize(&mut self, request: &Request, now: u64) -> Result<()> {
        if request.protocol_version != VERSION
            || request.installation_key != self.peer.installation_key
            || request.generation != self.peer.generation
            || request.session_id != self.session
            || !commands::opaque_id(&request.request_id)
            || request.deadline_ms <= now
            || request.deadline_ms.saturating_sub(now) > MAX_DEADLINE_MS
        {
            return Err("peer_request_denied");
        }
        // The command host is the only federated query/dispatch principal. Other
        // product roles acquire specific artifact/navigation capabilities separately.
        if self.peer.product != "control-center" && !matches!(request.call, Call::Describe {}) {
            return Err("peer_method_denied");
        }
        validate_call(&request.call)?;
        self.seen.retain(|_, expires| *expires > now);
        if self.seen.contains_key(&request.request_id) {
            return Err("peer_request_replayed");
        }
        if self.seen.len() >= MAX_PENDING {
            return Err("peer_request_limit");
        }
        self.seen
            .insert(request.request_id.clone(), request.deadline_ms);
        Ok(())
    }
}
pub fn validate_call(call: &Call) -> Result<()> {
    match call {
        Call::Query {
            query_id,
            mode,
            source,
            query,
            generation,
            context,
            ..
        } => {
            if matches!(source, Source::Commands) {
                commands::validate_query(query)?;
                if *mode != QueryMode::Name {
                    return Err("peer_query_invalid");
                }
            } else if query.len() > commands::MAX_QUERY_BYTES || query.chars().any(char::is_control)
            {
                return Err("peer_query_invalid");
            }
            if !commands::opaque_id(query_id)
                || *generation == 0
                || *generation > 9_007_199_254_740_991
                || context
                    .as_ref()
                    .is_some_and(|context| context.validate().is_err())
            {
                return Err("peer_query_invalid");
            }
        }
        Call::CancelQuery { query_id } if !commands::opaque_id(query_id) => {
            return Err("peer_query_invalid");
        }
        Call::PreviewCommand { request } | Call::OpenCommand { request } => {
            if !commands::opaque_id(&request.operation_id)
                || request.command_id.len() > 256
                || !commands::revision(&request.revision)
                || request
                    .context
                    .as_ref()
                    .is_some_and(|context| context.validate().is_err())
                || request
                    .selection_id
                    .as_ref()
                    .is_some_and(|id| !commands::opaque_id(id))
            {
                return Err("peer_command_invalid");
            }
        }
        Call::CommandStatus { operation_id } if !commands::opaque_id(operation_id) => {
            return Err("peer_command_invalid");
        }
        _ => {}
    }
    Ok(())
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|_| "peer_frame_invalid")?;
    if body.len() > MAX_FRAME_BYTES {
        return Err("peer_frame_limit");
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}
pub fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T> {
    if body.is_empty() || body.len() > MAX_FRAME_BYTES {
        return Err("peer_frame_limit");
    }
    serde_json::from_slice(body).map_err(|_| "peer_frame_invalid")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            protocol_version: 1,
            installation_key: "a".repeat(64),
            generation: "b".repeat(64),
            session_id: "native-session".into(),
            request_id: "request".into(),
            deadline_ms: 2000,
            call: Call::Query {
                query_id: "query".into(),
                mode: QueryMode::Name,
                source: Source::Commands,
                query: "terminal".into(),
                generation: 1,
                context: None,
            },
        }
    }
    #[test]
    fn observed_peer_role_scope_generation_deadline_and_replay_are_independent() {
        let mut guard = Guard::new(
            Peer::from_native("control-center", &"a".repeat(64), &"b".repeat(64)).unwrap(),
            "native-session",
        )
        .unwrap();
        let mut wrong = request();
        wrong.installation_key = "c".repeat(64);
        assert_eq!(guard.authorize(&wrong, 1000), Err("peer_request_denied"));
        wrong = request();
        wrong.generation = "c".repeat(64);
        assert_eq!(guard.authorize(&wrong, 1000), Err("peer_request_denied"));
        wrong = request();
        wrong.session_id = "old-session".into();
        assert_eq!(guard.authorize(&wrong, 1000), Err("peer_request_denied"));
        guard.authorize(&request(), 1000).unwrap();
        assert_eq!(
            guard.authorize(&request(), 1000),
            Err("peer_request_replayed")
        );
        let mut product = Guard::new(
            Peer::from_native("knowledge", &"a".repeat(64), &"b".repeat(64)).unwrap(),
            "native-session",
        )
        .unwrap();
        assert_eq!(
            product.authorize(&request(), 1000),
            Err("peer_method_denied")
        );
    }
    #[test]
    fn executable_strings_and_raw_payloads_are_not_wire_methods() {
        let mut value = serde_json::to_value(request()).unwrap();
        value["call"] = serde_json::json!({"method":"exec","args":{"command":"synthetic"}});
        assert!(decode::<Request>(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(decode::<Request>(&vec![b' '; MAX_FRAME_BYTES + 1]).is_err());
        let frame = encode(&request()).unwrap();
        assert_eq!(
            u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
            frame.len() - 4
        );
        assert!(decode::<Request>(&frame[4..]).is_ok());
    }
}
