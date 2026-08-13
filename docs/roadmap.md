# Roadmap

10개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.
처음 8개 앱(port-manager~devbox-manager)을 완성한 뒤 에디터(code-pad)와 예약 실행·서비스 관리자(run-manager)를 추가했다.

## Phase 1 — Tauri 기본기 ✅
- [x] **port-manager** — IPC, Rust 기초, netstat 파싱, 포트/프로세스 관리
- [x] **developer-toolbox** — 사이드바 UI, 소형 도구 14종 (hash/uuid/regex/diff는 Rust)

## Phase 2 — 시스템/네트워크 ✅
- [x] **api-playground** — HTTP(reqwest), 요청 빌더, 응답 뷰어, history
- [x] **everything-plus** — FTS5 이름 인덱스/검색, 루트 관리, 백그라운드 재인덱스

## Phase 3 — 개인 데이터 플랫폼 ✅
- [x] **knowledge-base** — frontmatter/태그, 파일 저장소, FTS5 검색, 데일리 노트
- [x] **life-log** — 활동·git 집계 허브 (activity-timeline 흡수)

## 추가 앱 ✅
- [x] **wsl-desktop** — 임베디드 WSL 터미널 (분할 레이아웃, 동시 명령, wsl-dashboard 흡수)
- [x] **devbox-manager** — 앱 버전 체크·설치·업데이트·실행
- [x] **code-pad** — CodeMirror 6 경량 코드 에디터. 언어 중립 LSP 클라이언트와 Windows 로컬 stdio 서버 관리
  (진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프)
- [x] **run-manager** — 예약 실행(크론 잡)과 상시 실행(서비스)을 한곳에서 관리. Windows/WSL 실행 어댑터,
  DPAPI 환경변수 보호, 회전 로그 tail, 실패 알림, 서비스 재시작 정책·헬스체크

## 다음 작업

향후 작업은 [docs/product-opportunities.md](./product-opportunities.md) §17의 실행 계획
(PR 1~39 + Stage 4·5)을 따른다.

```
Stage -1   결정을 문서에 고정 (PR 1)
Stage 0a   통폐합·네이밍 (PR 2~4) — identifier com.devbox.*, 12개 → 10개
Stage 0b   배포 정상화 (PR 5~13) — 버전 단일 원본, 카탈로그, release manifest, Manager 안전성
Stage 0.5  공용 프리미티브 (PR 14~17) — crates/wsl, crates/search, packages/tokens, CSP
Stage 1    정확성·privacy (PR 18~25) — Everything+ watcher, Life Log idle/privacy, Knowledge CodeMirror
Stage 2    앱 간 연동 (PR 26~30) — integration snapshot, ProjectProfile, crates/integration
Stage 3    기존 앱 깊이 (PR 31~39) — Run Manager 관찰성, Code Pad 복구, API Playground env/secret
Stage 4    Workbench — ProjectProfile 기반 orchestration 앱
Stage 5    Webhook Lab, Dev Environment Doctor, Log Lens/Repo Manager
```

## 현재 상태
- 10개 앱 모두 WSL에서 구현 완료 (Rust 유닛 테스트 + clippy + 프론트 빌드 통과)
- 각 앱은 기능 단위 PR로 main에 머지됨
- Windows 실제 실행/배포는 Releases(v0.3.0)로 제공 중
