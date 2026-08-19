# Roadmap

13개 앱을 순차적으로 완성하고, 공통 코드가 실제로 필요해지면 그때 `crates/`·`packages/`로 추출한다.
처음 8개 앱(port-manager~devbox-manager)을 완성한 뒤 에디터(code-pad)·예약 실행·서비스 관리자(run-manager),
그리고 Stage 4·5 앱(workbench·webhook-lab·repo-manager)을 추가했다.

## Phase 1 — Tauri 기본기 ✅
- [x] **port-manager** — IPC, Rust 기초, netstat 파싱, 포트/프로세스 관리
- [x] **developer-toolbox** — 사이드바 UI, 소형 도구 13종 (hash/uuid/regex/diff는 Rust)

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

## Stage 4 — Workbench ✅
- [x] **workbench** — 프로젝트 기반 orchestration 셸. ProjectProfile(기존 두 저장소 흡수), Git/WSL/포트/서비스 사전 점검,
  Run Manager·WSL Desktop·Code Pad 시작, idempotent 실행 기록, `Stop What I Started`

## Stage 5 — 신규 앱 ✅
- [x] **webhook-lab** — 로컬 웹훅/콜백 서버 (inbound HTTP). request history, 응답 rule·delay·오류 재현,
  민감 헤더 masking, LAN 공개 기본 차단 (JSON fixture·API Playground 변환은 설계 문서의 향후 항목)
- [x] **dev environment doctor** — devbox-manager의 환경 진단 탭 (WSL/git/node/pnpm/rustc/cargo/devbox-data/catalog-ids)
- [x] **repo-manager** — Git repository 탐색·브랜치/worktree/상태 목록, worktree 생성, Code Pad·WSL Desktop·Workbench로 열기
  (파괴적 기본 동작 없음, remove 전 uncommitted/untracked 검사)

## 다음 단계 후보 (backlog)

`docs/product-opportunities.md` §17(PR 1~39 + Stage 4/5)은 **전부 완료**됐다. 이후 작업은
설계 문서 3종을 따른다.

| 문서 | 범위 |
|---|---|
| [wsl-desktop 터미널 설계](./superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md) | PTY 전송 결함, 클립보드·단축키, 레이아웃 복원, 멀티플렉서 opt-in |
| [앱 간 연동 설계](./superpowers/specs/2026-08-17-app-interop-design.md) | argv 계약, 카탈로그 capability, 스냅샷 버스 정리 |
| [UX 개선 설계](./superpowers/specs/2026-08-15-ux-improvements-design.md) | 컨텍스트 메뉴 13개 앱, toolbox 도구, 앱별 항목, 실사용 피드백 |

### v0.4.1 — 핫픽스 (결함만, 기능 추가 없음; 코드 구현 완료; Windows 수동 검증은 별도 체크리스트)

1. **wsl-desktop 터미널 출력·세션 실행 결함** — v0.4.0에서 `terminal.rs`가 PTY 읽기마다
   `String::from_utf8_lossy`를 호출해 읽기 경계의 한글·박스드로잉을 U+FFFD로 치환했고,
   `windowsPty` 미설정·팬 1×2 붕괴·영구 resize desync와 사용자 제보의 `bash -lc "cd ... && exec bash"`
   단일 argv 문제가 관찰됐다. v0.4.1에서 UTF-8 carry, `windowsPty`/resize, `wsl.exe ... --cd <cwd> --`
   분리 argv를 적용해 해결했다.
2. **작동하지 않는 앱 간 링크** — v0.4.0에서는 repo-manager와 workbench가 다른 앱에 인자를 넘겨도
   **argv를 읽는 앱이 없어** 빈 앱이 열렸다. v0.4.1에서 `crates/applink`와 Code Pad/WSL Desktop/
   Workbench의 수신 및 대상 매핑을 구현해 복구했다.

코드 구현과 WSL 검증은 완료됐고, Windows 실기 검증 매트릭스는 별도 수동 체크리스트로 관리한다.

### v0.5.0

1. 실사용 피드백 저비용분 (code-pad 프리뷰 구분, webhook-lab 라벨·curl, 설치 경로 표시)
2. **카탈로그 capability + 런타임 배포** — "다른 앱으로 열기"를 선언에서 생성
3. **컨텍스트 메뉴 13개 앱** — 공용 `packages/context-menu`(껍데기) + 앱별 항목(각 앱 소유)
4. 터미널 클립보드·사용성·세션 유지
5. developer-toolbox 도구 확장 / api-playground 헤더·업로드
6. 스냅샷 버스 정리 + 자동 발견
7. 실사용 피드백 잔여분 / knowledge-base 백링크

상세 항목·난이도·안전 경계·테스트 계획은 위 설계 문서 참조.

```
Stage -1   결정을 문서에 고정 (PR 1)                                  ✅
Stage 0a   통폐합·네이밍 (PR 2~4) — identifier com.devbox.*          ✅
Stage 0b   배포 정상화 (PR 5~13) — 버전 단일 원본, 카탈로그, manifest  ✅
Stage 0.5  공용 프리미티브 (PR 14~17) — crates/wsl·search, packages/tokens, CSP ✅
Stage 1    정확성·privacy (PR 18~25)                                  ✅
Stage 2    앱 간 연동 (PR 26~30) — integration snapshot, ProjectProfile ✅
Stage 3    기존 앱 깊이 (PR 31~39) — Run Manager 관찰성, Code Pad 복구 ✅
Stage 4    Workbench — ProjectProfile 기반 orchestration 앱          ✅
Stage 5    Webhook Lab, Dev Environment Doctor, Repo Manager          ✅
v0.4.1     핫픽스 — 터미널 PTY 전송 결함, 끊긴 앱 간 링크             ◐ (Windows 수동 검증 별도)
v0.5.0     유기성(argv 계약·카탈로그) + 컨텍스트 메뉴 + 터미널 사용성  ◻
```

## 현재 상태
- 13개 앱 모두 WSL에서 구현 완료 (Rust 유닛 테스트 + clippy + 프론트 빌드 통과)
- 각 앱은 기능 단위 PR로 main에 머지됨
- v0.4.0 정식 배포 완료 (13개 앱)
- [통합 Windows 검증 체크리스트](https://github.com/jihoon22-lee/devbox/issues/176) — Windows 실기·패키지·프로토콜·경로·시각
  검증은 별도 수동 체크리스트로 관리한다.
