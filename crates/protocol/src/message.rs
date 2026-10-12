//! JSON 메시지 봉투. 메서드별 파라미터·결과는 RawValue로 그대로 싣는다.
use crate::frame::{Frame, FrameKind};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ClientKind {
    App,
    Cli,
    Mcp,
    Dev,
    Test,
}

#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    Hello {
        version: String,
        protocol: u32,
        client: ClientKind,
        instance: String,
    },
    Request {
        id: u32,
        method: String,
        #[ts(type = "unknown")]
        params: Box<RawValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        client_request_id: Option<String>,
    },
    Cancel {
        id: u32,
    },
    Subscribe {
        id: u32,
        topic: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        #[ts(type = "unknown")]
        params: Option<Box<RawValue>>,
    },
    Unsubscribe {
        sub_id: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct WireError {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_raw_option")]
    #[ts(optional, type = "unknown")]
    pub detail: Option<Box<RawValue>>,
}

#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Welcome {
        version: String,
        protocol: u32,
        daemon_id: String,
    },
    Response {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        #[ts(type = "unknown")]
        result: Option<Box<RawValue>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        error: Option<WireError>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        diagnostic_id: Option<String>,
    },
    Subscribed {
        id: u32,
        sub_id: u32,
        rev: String,
    },
    Event {
        sub_id: u32,
        rev: String,
        #[ts(type = "unknown")]
        payload: Box<RawValue>,
    },
    Resync {
        sub_id: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Control {
    ChannelOpen,
    ChannelClose,
    StreamCredit { stream: u32, bytes: u32 },
}

// A present JSON null is a payload; absence is the optional envelope field.
fn deserialize_raw_option<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(deserializer).map(Some)
}

// Internally tagged serde enums buffer fields, which loses RawValue support.
// Read the raw envelope first, then deserialize its selected variant directly.
macro_rules! deserialize_envelope {
    ($name:ident { $( $tag:literal => $variant:ident { $( $(#[$attr:meta])* $field:ident : $ty:ty ),* $(,)? } ),* $(,)? }) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = Box::<RawValue>::deserialize(deserializer)?;
                #[derive(Deserialize)]
                struct Tag { #[serde(rename = "type")] kind: String }
                let tag: Tag = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                match tag.kind.as_str() {
                    $( $tag => {
                        #[derive(Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Fields { $( $(#[$attr])* $field: $ty ),* }
                        let fields: Fields = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                        Ok(Self::$variant { $( $field: fields.$field ),* })
                    } ),*
                    other => Err(serde::de::Error::unknown_variant(other, &[$( $tag ),*])),
                }
            }
        }
    };
}

deserialize_envelope!(ClientMessage {
    "hello" => Hello { version: String, protocol: u32, client: ClientKind, instance: String },
    "request" => Request { id: u32, method: String, params: Box<RawValue>, client_request_id: Option<String> },
    "cancel" => Cancel { id: u32 },
    "subscribe" => Subscribe { id: u32, topic: String, #[serde(default, deserialize_with = "deserialize_raw_option")] params: Option<Box<RawValue>> },
    "unsubscribe" => Unsubscribe { sub_id: u32 },
});

deserialize_envelope!(ServerMessage {
    "welcome" => Welcome { version: String, protocol: u32, daemon_id: String },
    "response" => Response { id: u32, #[serde(default, deserialize_with = "deserialize_raw_option")] result: Option<Box<RawValue>>, error: Option<WireError>, diagnostic_id: Option<String> },
    "subscribed" => Subscribed { id: u32, sub_id: u32, rev: String },
    "event" => Event { sub_id: u32, rev: String, payload: Box<RawValue> },
    "resync" => Resync { sub_id: u32 },
});

pub fn json_frame<T: Serialize>(channel: u32, msg: &T) -> Frame {
    let body = serde_json::to_vec(msg).expect("protocol messages always serialize");
    Frame {
        kind: FrameKind::Json,
        channel,
        body: Bytes::from(body),
    }
}

pub fn control_frame(channel: u32, msg: &Control) -> Frame {
    let mut f = json_frame(channel, msg);
    f.kind = FrameKind::Control;
    f
}

pub fn parse_json<T: for<'de> Deserialize<'de>>(f: &Frame) -> Result<T, serde_json::Error> {
    serde_json::from_slice(&f.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameKind;
    use serde::{Deserialize, Serialize};

    use serde_json::Value;

    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../fixtures/envelopes.json")).expect("fixture json")
    }
    fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(v: &Value) {
        let parsed: T = serde_json::from_str(&serde_json::to_string(v).expect("fixture text"))
            .expect("parse fixture");
        let back = serde_json::to_value(&parsed).expect("serialize");
        assert_eq!(&back, v);
    }

    #[test]
    fn every_fixture_round_trips_exactly() {
        let f = fixtures();
        for (_, v) in f["client"].as_object().expect("fixture JSON is an object") {
            round_trip::<ClientMessage>(v);
        }
        for (_, v) in f["server"].as_object().expect("fixture JSON is an object") {
            round_trip::<ServerMessage>(v);
        }
        for (_, v) in f["control"].as_object().expect("fixture JSON is an object") {
            round_trip::<Control>(v);
        }
    }

    #[test]
    fn json_frame_carries_channel_and_body() {
        let f = json_frame(6, &Control::ChannelOpen);
        assert_eq!(f.kind, FrameKind::Json);
        assert_eq!(f.channel, 6);
        assert_eq!(
            parse_json::<Control>(&f).expect("parse fixture message"),
            Control::ChannelOpen
        );
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use ts_rs::TS;

    #[test]
    fn raw_payloads_keep_precision_and_original_json() {
        let text = r#"{"type":"request","id":4294967295,"method":"test.raw","params":{ "n": 9007199254740993, "nested": [null,true] }}"#;
        let msg: ClientMessage = serde_json::from_str(text).expect("parse fixture JSON");
        let ClientMessage::Request {
            params,
            client_request_id,
            ..
        } = &msg
        else {
            panic!("request")
        };
        assert_eq!(
            params.get(),
            r#"{ "n": 9007199254740993, "nested": [null,true] }"#
        );
        assert!(client_request_id.is_none());
        assert!(
            serde_json::to_string(&msg)
                .expect("serialize fixture JSON")
                .contains(params.get())
        );
        let error: WireError =
            serde_json::from_str(r#"{"code":"test.raw","detail":{"n":9007199254740993}}"#)
                .expect("parse fixture JSON");
        assert_eq!(
            error.detail.expect("fixture error includes details").get(),
            r#"{"n":9007199254740993}"#
        );
    }

    #[test]
    fn optional_raw_values_preserve_present_null_and_nested_results() {
        for text in [
            r#"{"type":"response","id":1,"result":null}"#,
            r#"{"type":"response","id":1,"result":{"n":9007199254740993}}"#,
            r#"{"type":"response","id":1,"error":{"code":"test.x","detail":null}}"#,
            r#"{"type":"event","subId":2,"rev":"9007199254740993","payload":null}"#,
        ] {
            let parsed: ServerMessage = serde_json::from_str(text).expect("parse fixture JSON");
            assert_eq!(
                serde_json::to_string(&parsed).expect("serialize fixture JSON"),
                text
            );
        }
        let text = r#"{"type":"subscribe","id":1,"topic":"test.x","params":null}"#;
        let parsed: ClientMessage = serde_json::from_str(text).expect("parse fixture JSON");
        assert_eq!(
            serde_json::to_string(&parsed).expect("serialize fixture JSON"),
            text
        );
    }

    #[test]
    fn wire_ids_are_numbers_and_revisions_are_strings() {
        for text in [
            r#"{"type":"cancel","id":"3"}"#,
            r#"{"type":"cancel","id":-1}"#,
            r#"{"type":"cancel","id":4294967296}"#,
            r#"{"type":"unknown"}"#,
            r#"{"type":"request","id":1,"method":"x"}"#,
        ] {
            assert!(
                serde_json::from_str::<ClientMessage>(text).is_err(),
                "{text}"
            );
        }
        assert!(
            serde_json::from_str::<ServerMessage>(
                r#"{"type":"subscribed","id":5,"subId":2,"rev":41}"#
            )
            .is_err()
        );
    }

    #[test]
    fn frame_helpers_use_existing_codec_and_parse_reports_bad_json() {
        use crate::frame::{Frame, FrameCodec, FrameKind};
        use bytes::{Bytes, BytesMut};
        use tokio_util::codec::{Decoder, Encoder};
        let expected = Control::StreamCredit {
            stream: 7,
            bytes: 262144,
        };
        let frame = control_frame(42, &expected);
        assert_eq!(frame.kind, FrameKind::Control);
        let mut encoded = BytesMut::new();
        FrameCodec::default()
            .encode(frame, &mut encoded)
            .expect("encode fixture frame");
        let decoded = FrameCodec::default()
            .decode(&mut encoded)
            .expect("decode fixture frame")
            .expect("decode fixture frame");
        assert_eq!(decoded.channel, 42);
        assert_eq!(
            parse_json::<Control>(&decoded).expect("parse fixture message"),
            expected
        );
        let bad = Frame {
            kind: FrameKind::Json,
            channel: 0,
            body: Bytes::from_static(b"{"),
        };
        assert!(parse_json::<ClientMessage>(&bad).is_err());
    }

    #[test]
    fn generated_types_match_serde_camel_case_and_optional_raw_values() {
        let config = ts_rs::Config::new();
        let client = ClientMessage::decl(&config);
        let server = ServerMessage::decl(&config);
        let error = WireError::decl(&config);
        assert!(client.contains("clientRequestId?: string"), "{client}");
        assert!(client.contains("subId: number"), "{client}");
        assert!(client.contains("params: unknown"), "{client}");
        assert!(server.contains("diagnosticId?: string"), "{server}");
        assert!(server.contains("rev: string"), "{server}");
        assert!(error.contains("detail?: unknown"), "{error}");
    }
}
