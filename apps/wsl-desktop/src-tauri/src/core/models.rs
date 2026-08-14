use serde::Serialize;

/// WSL 배포판 정보
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DistroInfo {
    pub name: String,
    pub version: u32,
    pub default: bool,
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

/// git 저장소 상태
#[derive(Debug, Clone, Serialize)]
pub struct GitStatus {
    pub path: String,
    pub branch: String,
    pub changes: u32,
    pub clean: bool,
}
