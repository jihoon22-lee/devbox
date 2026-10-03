# Devbox

Windows용 네 제품을 제공하는 Tauri v2·React·Rust 모노레포입니다.

| 제품 | 포함 기능 |
|---|---|
| **Workspace** | 프로젝트·worktree·에이전트 작업·파일 편집·Windows/WSL LSP·Git·의존성·세션·작업·포트·로그·Problems·터미널 |
| **API Studio** | Requests·가져오기·컬렉션 실행기·OAuth 2.0/TLS·Protocols·Webhooks·Transforms·response diff·mock·선택한 결과의 Knowledge 전달 |
| **Knowledge** | Notes·Daily·Activity·파일/노트 통합 검색·vault·templates·저장 검색과 제외 설정 |
| **Control Center** | 제품 관리·Command/Shortcut 검색·도구·진단·백그라운드 서비스/MCP 설정·Suite 설치/업데이트/복구 |

## 다운로드와 설치

v0.9.0 철회본은 2026-10-03에 제거했으며 수정본의 재출시를 준비하고 있습니다.
실제 다운로드 가능한 버전은 [GitHub Releases](https://github.com/jihoon22-lee/devbox/releases),
검증 진행 상황은 [현재 계획](docs/superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)과
[릴리스 근거](docs/release-evidence.md)에서 확인하세요.

Suite 배포는 `Devbox_0.9.0_x64-setup.exe` 하나와 제품별 portable ZIP 네 개,
`release-manifest.json`, `THIRD_PARTY_NOTICES.md`로 구성됩니다. ZIP은 폴더 전체를
풀어 실행하세요. Workspace WSL helper와 Control Center 설치 helper를 떼어내지 마세요.
v0.9.0에는 v0.7 데이터 가져오기가 없습니다. v0.7 데이터는 [v0.8.1](https://github.com/jihoon22-lee/devbox/releases/tag/v0.8.1)에서
먼저 이전해야 합니다. v0.8.1의 내장 업데이터로는 v0.9.0을 설치할 수 없습니다.
Release에서 setup을 직접 받아 실행하고, 기존 설치 폴더와 데이터를 삭제하지 마세요.
검증 범위와 알려진 제한은 [Windows 안내](docs/windows-guide.md#v090-설치와-알려진-제한)를 확인하세요.

## 문서

- [아키텍처 결정 기록](docs/adr/README.md)

- [Windows 설치·이전·복구](docs/windows-guide.md)
- [제품과 내부 모듈](docs/projects.md) · [아키텍처](docs/architecture.md)
- [개발](docs/development.md) · [공통 규약](CONVENTIONS.md)
- [v0.8 수용 추적](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md) · [개선 로드맵](docs/superpowers/plans/2026-10-03-product-readiness/00-roadmap.md)
- [릴리스 정책](docs/release-policy.md) · [현재 준비 상태와 릴리스 근거](docs/release-evidence.md)
- [v0.7 역사적 제품·배포 안내](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.7/README.md)
