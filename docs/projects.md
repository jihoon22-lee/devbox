# Projects

10개 앱의 요약. 상세 계획은 각 `apps/<AppName>/PLAN.md`를 참조한다.

| # | 앱 | 디렉터리 | 핵심 목적 | Phase | 연계 |
|---|---|---|---|---|---|
| 1 | port-manager | `apps/port-manager` | 포트·프로세스 조회/종료/실행 | 1 | process crate, run-manager |
| 2 | developer-toolbox | `apps/developer-toolbox` | 개발 소형 도구 모음 | 1 | packages/tokens |
| 3 | wsl-desktop | `apps/wsl-desktop` | 임베디드 WSL 터미널 (분할·동시 명령, wsl-dashboard 흡수) | 추가 | wsl crate, workbench |
| 4 | api-playground | `apps/api-playground` | REST API 테스트 | 2 | packages/tokens |
| 5 | everything-plus | `apps/everything-plus` | 로컬 파일 초고속 검색 | 2 | filesystem/search crate, code-pad |
| 6 | knowledge-base | `apps/knowledge-base` | 마크다운 지식 저장소 | 3 | filesystem/search crate, packages/editor |
| 7 | life-log | `apps/life-log` | 자동 일일 로그 (집계 허브, activity-timeline 흡수) | 3 | integration snapshot, workbench |
| 8 | devbox-manager | `apps/devbox-manager` | devbox 앱 설치·업데이트·실행 | 추가 | 전 앱 |
| 9 | code-pad | `apps/code-pad` | CodeMirror 6 경량 코드 에디터 (LSP) | 추가 | filesystem/markdown crate |
| 10 | run-manager | `apps/run-manager` | 예약 실행·서비스 관리 (cron + service) | 추가 | process crate, workbench |

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

## 산출물 (각각 독립 .exe)
`PortManager.exe` `DevToolbox.exe` `WSLDesktop.exe` `ApiPlayground.exe`
`EverythingPlus.exe` `Knowledge.exe` `LifeLog.exe` `DevboxManager.exe` `CodePad.exe` `RunManager.exe`
