# Projects

15개 구현 앱의 요약. 현재 v0.6.0 stable은 milestone #2의 W01~W11을 포함한 15개 앱
bundle이다. 상세 소개는 각
`apps/<AppName>/README.md`, 설계는 `docs/superpowers/specs/`를 참조한다.

## W08 PR2 (#489) 문서 계약

W08 PR2는 v0.6.0 stable에 포함된 integration 작업이다. Log Lens 0.2.0은
source 설정과 filter만 담는 strict app-local saved view(schema v1, 최대 20개)를 제공한다.
저장소는 revision CAS, atomic/no-link write를 사용하고 corrupt/oversized/unknown-field
내용을 보존한 채 fail-closed한다. WSL file source와 ephemeral Webhook capture는 저장하지
않으며, saved view를 불러오면 읽기를 끊고 사용자가 명시적으로 `source 재연결`해야 한다.

Webhook Lab 0.3.0은 catalog revision 17의 `webhook-log/v1` one-time producer다. Log Lens에는
method, redacted origin-form target, timestamp, header names, 최대 4 KiB redacted body
preview와 flags만 전달한다. Header values, raw body, filesystem path, command, environment,
credential, archive는 handoff와 argv에 포함하지 않으며 launch 실패 시 정확한 pending entry만
정리한다. Canonical wire `displayName`은 영어(`Webhook capture`)로 유지하고 UI는 한국어로
제공한다. exact-main candidate의 15-app packaged runtime과 v0.6.0 release 검증은 완료됐고,
설치 사용자 환경에서 남은 physical observation은 #518에서 별도로 관리한다.

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
| 12 | webhook-lab | `apps/webhook-lab` | 로컬 웹훅/콜백 서버 (0.3.0, `webhook-log/v1` producer) | Stage 5 | api-playground, port-manager, log-lens |
| 13 | repo-manager | `apps/repo-manager` | git 저장소·worktree 관리 | Stage 5 | wsl crate, code-pad/workbench |
| 14 | devbox-launcher | `apps/devbox-launcher` | catalog app과 제공될 때 검증된 integration snapshot 검색·AppLink 실행, explicit clipboard preview | P3-01 | catalog, integration, applink, launch |
| 15 | log-lens | `apps/log-lens` | local/WSL/container/Webhook log tail·merge·filter, Run/Webhook handoff·reader, saved views (0.2.0) | P3-02 | `log-source/v1`, `webhook-log/v1`, bounded in-memory ring |

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
| webhook-lab | http, rules, masking, `webhook-log/v1` producer |
| repo-manager | wsl, git |
| devbox-launcher | catalog, integration, applink, launch |
| log-lens | WSL/container fixed adapters, app-local parser, `log-source/v1`/`webhook-log/v1` claim/preview, fixed Run rotation reader, app-local saved views (#473; v0.5.0 binary 제외) |

## 산출물 (각각 독립 .exe)
`PortManager.exe` `DevToolbox.exe` `WSLDesktop.exe` `ApiPlayground.exe`
`EverythingPlus.exe` `Knowledge.exe` `LifeLog.exe` `DevboxManager.exe` `CodePad.exe` `RunManager.exe`
`Workbench.exe` `WebhookLab.exe` `RepoManager.exe` `DevboxLauncher.exe` `LogLens.exe`

## v0.5.0 신규 앱 구현 및 v0.5.1 stable 보강

Devbox Launcher와 Log Lens bootstrap은 공개 v0.5.0 stable에 포함됐다. #366/#367의
Run Manager·WSL Desktop producer와 Log Lens bounded claim/preview lifecycle도 유지한다.
완료 감사에서 발견한 Run source read 누락은 #472/#473에서 고정 app-data root·logical offset
기반 read-only adapter로 보완됐고, 이 보완은 v0.5.1 stable에 포함된다.
#479 merge로 닫힌 #474 계약도 v0.5.1에 포함된다. 기존 flat
`run-manager/v1/summary.json`은 유지하고, named `run-manager/v1/jobs-services.json` sidecar가
Launcher의 전체 job/service action을 제공한다. 정확한 v0.5.1 package evidence는 GitHub Release에서
확인한다. producer path나 Run Manager DB를 전달·직접 읽지 않으며,
기존 ancestor TOCTOU와 local-adapter FIFO/UNC reader 위험은 이 보완 범위에 포함하지 않는다.
기존 앱의 P1·P2·선택 P3 강화, 앱별 목표 version, 신규 앱의 안전 경계와 acceptance는
[v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)을 따른다.
