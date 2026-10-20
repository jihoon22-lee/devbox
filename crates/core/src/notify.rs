//! Small daemon notification contract; presentation is owned by the application.
use crate::Hub;
use devbox_protocol::{Exporter, topic};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub project_id: String,
    pub run_id: Option<String>,
    pub task: Option<String>,
    pub reason: NotificationReason,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum NotificationReason {
    StartLimit,
    #[serde(rename = "schedule_failed")]
    ScheduleFailed,
    #[serde(rename = "schedule_trust_required")]
    ScheduleTrustRequired,
}
topic!(pub NotifyRaised = "notify.raised", Notification);
pub fn raise(hub: &Hub, rev: u64, notification: &Notification) {
    hub.publish::<NotifyRaised>(rev, notification);
}
pub fn export<X: Exporter>(exporter: &mut X) {
    exporter.topic::<NotifyRaised>();
}
