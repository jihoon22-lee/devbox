//! WSL argv 조립과 경로 정규화 프리미티브.
//!
//! 추출 근거: 병합 후 두 앱(wsl-desktop, run-manager)이 WSL 명령을 구성한다.
//! §10.2 ProjectProfile의 canonical identity는 Windows 경로와 WSL 경로의
//! 정규화 규칙이 **하나**여야 성립한다. 이 크레이트는 그 규칙의 단일 원본이다.
//!
//! 제약 (CONVENTIONS.md §4): Windows 전용 코드 없음, 프로세스 실행 없음.

pub mod argv;
pub mod distro;
pub mod path;

use std::fmt;

/// WSL 프리미티브 오류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslError {
    /// distro 이름이 비었거나 argv 주입 가능한 문자가 있다.
    InvalidDistro(String),
    /// 경로가 드라이브/정규화 가능한 형태가 아니다.
    InvalidPath(String),
}

impl fmt::Display for WslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WslError::InvalidDistro(msg) => write!(f, "잘못된 WSL distro 이름: {msg}"),
            WslError::InvalidPath(msg) => write!(f, "잘못된 경로: {msg}"),
        }
    }
}

impl std::error::Error for WslError {}
