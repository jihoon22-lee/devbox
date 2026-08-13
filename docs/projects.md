# Projects

12개 앱의 요약. 상세 계획은 각 `apps/<AppName>/PLAN.md`를 참조한다.

| # | 앱 | 디렉터리 | 핵심 목적 | Phase | 연계 |
|---|---|---|---|---|---|
| 1 | port-manager | `apps/port-manager` | 포트·프로세스 조회/종료/실행 | 1 | process crate, wsl-dashboard |
| 2 | developer-toolbox | `apps/developer-toolbox` | 개발 소형 도구 모음 | 1 | packages/ui |
| 3 | wsl-dashboard | `apps/wsl-dashboard` | WSL·Docker·git 통합 관리 | 2 | process/wsl crate |
| 4 | api-playground | `apps/api-playground` | REST API 테스트 | 2 | packages/ui |
| 5 | activity-timeline | `apps/activity-timeline` | PC 사용 타임라인 | 3 | database/activity crate, life-log |
| 6 | everything-plus | `apps/everything-plus` | 로컬 파일 초고속 검색 | 3 | filesystem/search crate, knowledge-base |
| 7 | knowledge-base | `apps/knowledge-base` | 마크다운 지식 저장소 | 4 | filesystem/search crate, life-log |
| 8 | life-log | `apps/life-log` | 자동 일일 로그 (집계 허브) | 4 | 전 앱 데이터 |
| 9 | wsl-desktop | `apps/wsl-desktop` | 임베디드 WSL 터미널 (분할·동시 명령) | 추가 | wsl-dashboard |
| 10 | devbox-manager | `apps/devbox-manager` | devbox 앱 설치·업데이트·실행 | 추가 | 전 앱 |
| 11 | code-pad | `apps/code-pad` | CodeMirror 6 경량 코드 에디터 (LSP) | 추가 | filesystem/markdown crate |
| 12 | run-manager | `apps/run-manager` | 예약 실행·서비스 관리 (cron + service) | 추가 | process crate |

## 공유 후보 매트릭스

| 프로젝트 | 공유할 가능성이 높은 것 |
|---|---|
| port-manager | process, port, Windows API |
| wsl-dashboard | process, port, wsl, git |
| wsl-desktop | wsl, pty |
| developer-toolbox | ui, settings, clipboard |
| activity-timeline | process, database, Windows API |
| everything-plus | filesystem, database, search |
| knowledge-base | filesystem, database, search |
| api-playground | ui, settings, http |
| life-log | activity, database, filesystem, git |
| devbox-manager | http, update |
| code-pad | filesystem, markdown, (lsp — 두 번째 소비자 시 `crates/lsp`) |
| run-manager | process, database |

## 산출물 (각각 독립 .exe)
`PortManager.exe` `WSLDashboard.exe` `DevToolbox.exe` `ActivityTimeline.exe`
`EverythingPlus.exe` `Knowledge.exe` `ApiPlayground.exe` `LifeLog.exe`
`WSLDesktop.exe` `DevboxManager.exe` `CodePad.exe` `RunManager.exe`
