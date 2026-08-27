# Projects

15개 구현 앱의 요약. 상세 소개는 각 `apps/<AppName>/README.md`, 설계는 `docs/superpowers/specs/`를 참조한다.

| # | 앱 | 디렉터리 | 핵심 목적 | Phase | 연계 |
|---|---|---|---|---|---|
| 1 | port-manager | `apps/port-manager` | 포트·프로세스 조회/종료/실행 | 1 | process crate, run-manager |
| 2 | developer-toolbox | `apps/developer-toolbox` | 개발 소형 도구 모음 | 1 | packages/tokens |
| 3 | wsl-desktop | `apps/wsl-desktop` | 임베디드 WSL 터미널 (분할·동시 명령, wsl-dashboard 흡수) | 추가 | wsl crate, workbench |
| 4 | api-playground | `apps/api-playground` | REST API 테스트 | 2 | packages/tokens, secrets crate |
| 5 | everything-plus | `apps/everything-plus` | 로컬 파일 초고속 검색 | 2 | filesystem/search crate, code-pad |
| 6 | knowledge-base | `apps/knowledge-base` | 마크다운 지식 저장소 | 3 | filesystem/search crate, packages/editor |
| 7 | life-log | `apps/life-log` | 자동 일일 로그 (집계 허브, activity-timeline 흡수) | 3 | integration snapshot, workbench |
| 8 | devbox-manager | `apps/devbox-manager` | devbox 앱 설치·업데이트·실행 (+ 환경 진단) | 추가 | 전 앱 |
| 9 | code-pad | `apps/code-pad` | CodeMirror 6 경량 코드 에디터 (LSP) | 추가 | filesystem/markdown crate |
| 10 | run-manager | `apps/run-manager` | 예약 실행·서비스 관리 (cron + service) | 추가 | process crate, workbench |
| 11 | workbench | `apps/workbench` | 프로젝트 기반 orchestration 셸 | Stage 4 | wsl·integration crate, 전 앱 |
| 12 | webhook-lab | `apps/webhook-lab` | 로컬 웹훅/콜백 서버 | Stage 5 | api-playground, port-manager |
| 13 | repo-manager | `apps/repo-manager` | git 저장소·worktree 관리 | Stage 5 | wsl crate, code-pad/workbench |
| 14 | devbox-launcher | `apps/devbox-launcher` | catalog app과 제공될 때 검증된 integration snapshot 검색·AppLink 실행, explicit clipboard preview | P3-01 | catalog, integration, applink, launch |
| 15 | log-lens | `apps/log-lens` | local/Run/WSL/container log tail·merge·filter | P3-02 | `log-source/v1`, bounded in-memory ring |

## 공유 후보 매트릭스

| 프로젝트 | 공유할 가능성이 높은 것 |
|---|---|
| port-manager | process, Windows API |
| wsl-desktop | wsl, pty |
| developer-toolbox | tokens, settings, clipboard |
| everything-plus | filesystem, search |
| knowledge-base | filesystem, search, editor |
| api-playground | tokens, settings, http |
| life-log | database, filesystem, git, integration |
| devbox-manager | http, update, catalog |
| code-pad | filesystem, markdown, editor, (lsp — 두 번째 소비자 시 `crates/lsp`) |
| run-manager | process, database, wsl |
| workbench | wsl, integration, catalog |
| webhook-lab | http, rules, masking |
| repo-manager | wsl, git |
| devbox-launcher | catalog, integration, applink, launch |
| log-lens | WSL fixed adapters, app-local parser, `log-source/v1` |

## 산출물 (각각 독립 .exe)
`PortManager.exe` `DevToolbox.exe` `WSLDesktop.exe` `ApiPlayground.exe`
`EverythingPlus.exe` `Knowledge.exe` `LifeLog.exe` `DevboxManager.exe` `CodePad.exe` `RunManager.exe`
`Workbench.exe` `WebhookLab.exe` `RepoManager.exe` `DevboxLauncher.exe` `LogLens.exe`

## v0.5.0 신규 앱 진행 상태

Devbox Launcher와 Log Lens bootstrap은 구현됐다. Log Lens의 Run/WSL producer handoff는
별도 integration PR에서 연결한다.
기존 앱의 P1·P2·선택 P3 강화, 앱별 목표 version, 신규 앱의 안전 경계와 acceptance는
[v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)을 따른다.
