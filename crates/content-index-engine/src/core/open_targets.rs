use serde::Serialize;
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EverythingOpenTarget {
    pub id: String,
    pub display_name: String,
}
