# Roadmap

12개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.
처음 10개 앱(port-manager~devbox-manager)을 완성한 뒤 에디터(code-pad)와 예약 실행·서비스 관리자(run-manager)를 추가했다.

## Phase 1 — Tauri 기본기 ✅
- [x] **port-manager** — IPC, Rust 기초, netstat 파싱, 포트/프로세스 관리
- [x] **developer-toolbox** — 사이드바 UI, 소형 도구 14종 (hash/uuid/regex/diff는 Rust)

## Phase 2 — 시스템/네트워크 ✅
- [x] **wsl-dashboard** — 자식 프로세스, async 명령, wsl/docker/git 파싱
- [x] **api-playground** — HTTP(reqwest), 요청 빌더, 응답 뷰어, history

## Phase 3 — 데이터/검색 ✅
- [x] **activity-timeline** — SQLite, 세션 병합, 트레이, foreground window 추적
- [x] **everything-plus** — FTS5 이름 인덱스/검색, 루트 관리, 백그라운드 재인덱스

## Phase 4 — 개인 데이터 플랫폼 ✅
- [x] **knowledge-base** — frontmatter/태그, 파일 저장소, FTS5 검색, 데일리 노트
- [x] **life-log** — 활동·git 집계 허브 (activity DB + 커밋 수)

## 추가 앱 ✅
- [x] **wsl-desktop** — 임베디드 WSL 터미널 (분할 레이아웃, 동시 명령)
- [x] **devbox-manager** — 앱 버전 체크·설치·업데이트·실행
- [x] **code-pad** — CodeMirror 6 경량 코드 에디터. 언어 중립 LSP 클라이언트와 Windows 로컬 stdio 서버 관리
  (진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프)
- [x] **run-manager** — 예약 실행(크론 잡)과 상시 실행(서비스)을 한곳에서 관리. Windows/WSL 실행 어댑터,
  DPAPI 환경변수 보호, 회전 로그 tail, 실패 알림, 서비스 재시작 정책·헬스체크

## 후속 작업
- [ ] **공통 추출** — `crates/process`(포트/프로세스), `crates/database`, `crates/search` 등 2개 앱 이상에서
  실제 중복이 확인되면 추출. 현재 `crates/filesystem`·`crates/markdown`·`crates/process` 추출 완료
- [ ] **everything-plus** — 파일 watcher(실시간 증분), 내용 인덱싱 최적화
- [ ] **activity-timeline** — 유휴 감지, 자동 시작
- [ ] **Windows 빌드 검증** — 12개 앱 `pnpm tauri build` (MSVC 툴체인)
- [ ] **통합 앱 (선택)** — `apps/workbench` 대시보드

## 현재 상태
- 12개 앱 모두 WSL에서 구현 완료 (Rust 유닛 테스트 + clippy + 프론트 빌드 통과)
- 각 앱은 기능 단위 PR로 main에 머지됨
- Windows 실제 실행/배포는 Releases(v0.3.0)로 제공 중
