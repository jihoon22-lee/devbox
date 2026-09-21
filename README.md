# Devbox

Windows용 네 제품을 제공하는 Tauri v2·React·Rust 모노레포입니다.

| 제품 | 포함 기능 |
|---|---|
| **Workspace** | 프로젝트·worktree·파일 편집·Windows/WSL LSP·Git·의존성·세션·작업·포트·로그·Problems·터미널 |
| **API Studio** | Requests·Protocols·Webhooks·Transforms·response diff·mock·선택한 결과의 Knowledge 전달 |
| **Knowledge** | Notes·Daily·Activity·파일/노트 통합 검색·vault·templates·저장 검색과 제외 설정 |
| **Control Center** | 제품 관리·Command/Shortcut 검색·도구·진단·데이터 이전·Suite 설치/업데이트/복구 |

## 다운로드와 설치

[공개 Releases](https://github.com/jihoon22-lee/devbox/releases)의 실제 게시 상태를 확인하세요.
v0.8 소스 전환과 공개 완료는 구분하며, 현재 진행 및 최종 후보·배포 근거는
[v0.8 원장 #541](https://github.com/jihoon22-lee/devbox/issues/541)에 기록합니다.

v0.8 배포는 `Devbox_0.8.1_x64-setup.exe` 하나와 제품별 portable ZIP 네 개,
`release-manifest.json`, `THIRD_PARTY_NOTICES.md`로 구성됩니다. ZIP은 폴더 전체를
풀어 실행하세요. Workspace WSL helper와 Control Center 설치 helper를 떼어내지 마세요.
기존 v0.7 데이터는 명시적인 이전 검토 후 새 namespace로 복사하며 원본을 유지합니다.

## 문서

- [Windows 설치·이전·복구](docs/windows-guide.md)
- [제품과 내부 모듈](docs/projects.md) · [아키텍처](docs/architecture.md)
- [개발](docs/development.md) · [공통 규약](CONVENTIONS.md)
- [v0.8 수용 추적](docs/v0.8-acceptance.md) · [로드맵](docs/roadmap.md)
- [릴리스 정책](docs/release-policy.md) · [과거 릴리스 근거](docs/release-evidence.md)
- [v0.7 역사적 제품·배포 안내](docs/history/v0.7/README.md)
