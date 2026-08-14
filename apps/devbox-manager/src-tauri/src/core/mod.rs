// TODO(0.4.0): PR 11(fix/devbox-manager/per-app-versions)에서 명령 계층이 이 모듈을
// 소비하기 전까지는 순수 로직만 있고 호출부가 없다. 그 전까지 dead_code를 허용한다.
#![allow(dead_code)]

pub mod asset;
pub mod catalog;
pub mod download;
pub mod layout;
pub mod manifest;
pub mod url_policy;
pub mod version;
