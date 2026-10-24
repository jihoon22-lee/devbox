//! `api.collection*` RPC 경계 타입. 파일 내용은 텍스트로만 오간다(스키마는 화면 TS가 소유).
/// 루트(개인 = `Paths.config_dir/api`, 프로젝트 = `<Project.path>/.devbox/api`)는 S5-2 `CollectionRoot`를 그대로 쓴다.
pub use crate::model::CollectionRoot;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionsQuery {
    pub root: CollectionRoot,
}

/// 목록 항목. `path`는 루트 기준 `/` 구분 상대 경로, 1MiB를 넘는 파일은 `digest`가 없다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionFile {
    pub path: String,
    pub size: i64,
    pub digest: Option<String>,
    /// 프로젝트 파일이 비밀 참조·파일 부분·stdio 명령을 쓰면 이 PC에서 그 digest를 검토했을 때만 참.
    pub reviewed: bool,
    /// 권한 등으로 읽지 못한 파일. 목록 전체를 실패시키지 않고 이 항목만 표시한다.
    pub unreadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionList {
    pub root: CollectionRoot,
    pub items: Vec<CollectionFile>,
    /// 루트당 2,000개를 넘으면 참.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileRef {
    pub root: CollectionRoot,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub text: String,
    pub digest: String,
    pub reviewed: bool,
}

/// 쓰기 조건: 읽은 digest 그대로일 때만(저장) 또는 없을 때만(새로 저장·가져오기).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CollectionExpect {
    Digest { digest: String },
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFile {
    pub root: CollectionRoot,
    pub path: String,
    pub text: String,
    pub expect: CollectionExpect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub digest: String,
    pub rev: String,
}

/// 삭제도 화면이 본 digest와 같을 때만 한다(밖에서 고친 파일을 지우지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteFile {
    pub root: CollectionRoot,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFile {
    pub root: CollectionRoot,
    pub path: String,
    pub digest: String,
}

/// 루트 아래 파일이 바뀜(300ms 디바운스). 화면은 목록을 다시 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionsChange {
    pub root: CollectionRoot,
    pub rev: String,
}
