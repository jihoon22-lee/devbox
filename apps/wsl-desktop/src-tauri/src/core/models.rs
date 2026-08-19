use serde::Serialize;

/// WSL 배포판 정보
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DistroInfo {
    pub name: String,
    pub version: u32,
    pub default: bool,
    /// `wsl.exe -l -v`의 STATE 컬럼 (`Running`/`Stopped` 등). 원문 그대로 보관한다 —
    /// 지역화·대소문자 정규화가 필요하면 소비하는 쪽에서 한다.
    pub state: String,
}

/// Docker 컨테이너 정보
#[derive(Debug, Clone, Serialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub ports: String,
}
