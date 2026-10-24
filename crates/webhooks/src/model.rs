use crate::ResponseRule;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookEmpty")]
pub struct Empty {}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookStartListener")]
pub struct StartListener {
    pub port: u16,
    pub lan: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookListener")]
pub struct Listener {
    pub id: String,
    pub port: u16,
    pub lan: bool,
    pub addresses: Vec<String>,
    pub running: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookListenerList")]
pub struct ListenerList {
    pub items: Vec<Listener>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookListenerId")]
pub struct ListenerId {
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookUpsertRule")]
pub struct UpsertRule {
    pub listener_id: String,
    pub rule: ResponseRule,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookRuleList")]
pub struct RuleList {
    pub items: Vec<ResponseRule>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookRuleRef")]
pub struct RuleRef {
    pub listener_id: String,
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookReceived")]
pub struct Received {
    pub listener_id: String,
    pub id: String,
    pub method: String,
    pub path: String,
    pub size_bytes: u32,
    pub at_ms: f64,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookHistoryQuery")]
pub struct HistoryQuery {
    pub listener_id: String,
    pub before: Option<String>,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookReceivedPage")]
pub struct ReceivedPage {
    pub items: Vec<Received>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookReceivedId")]
pub struct ReceivedId {
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookReceivedDetail")]
pub struct ReceivedDetail {
    pub received: Received,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
    pub preview_truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "WebhookSaveFixture")]
pub struct SaveFixture {
    pub received_id: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(rename = "WebhookFixture")]
pub struct Fixture {
    pub id: String,
    pub name: String,
    pub request: ReceivedDetail,
}
