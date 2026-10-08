# 진행 원장

devbox v1 재구축의 진행 상태를 적는 **유일한 원장**이다. 계획 문서의 체크박스와 상태 줄은 보조다.
세션을 시작하고 끝낼 때 [00-roadmap §3.1](00-roadmap.md)의 인계 절차대로 이 파일을 읽고 고친다.

**읽는 법**
- 이 파일은 PR과 함께 main에 들어간다. 작업 중인 브랜치가 있으면 **그 브랜치의 이 파일이 더 최신**이다(§3.1 2단계에서 찾는다).
- 상태 값: `대기` · `작업 중` · `PR 열림` · `머지` · `막힘`(사유를 "막힌 것"에 적음).
- 날짜는 `YYYY-MM-DD`, 시각이 필요하면 KST.

## 1. 현재 위치

| 항목 | 값 |
|---|---|
| 단계 | P0 완료 · S0a 대기 |
| 다음 할 일 | 사용자가 실행을 지시하면 S0a Task 1 |
| 작업 중인 브랜치·worktree | `docs/v1-plan` · `../devbox-wt/docs-v1-plan` — PR A 전까지 이 계획의 원본(00-roadmap §2 P0) |
| 마지막 갱신 | 2026-10-08 · 통합 검토 세션 |

## 2. 막힌 것 · 사용자 결정 대기

| 날짜 | 무엇 | 누구 결정 | 상태 |
|---|---|---|---|
| 2026-10-08 | P0 진행 방식 | 사용자 | 해결: PR·CI 없이 `docs/v1-plan`에 커밋·push, main에는 PR A와 함께 |
| 2026-10-08 | 이 PC의 로컬 main에 원격에 없는 커밋 2개(`b2a508b6` source-map-js 갱신, `1b84aa9d` 완료 계획 정리, 둘 다 2026-10-07). `docs/v1-plan`은 원격 main(`72ff50c7`)에서 갈라졌다. 두 커밋의 내용은 PR A(옛 코드·문서 전부 삭제)로 의미가 없어진다. PR A 머지 뒤 로컬 main을 원격에 맞출지(두 커밋 버림), 그 전에 PR로 올릴지 | 사용자 | 대기 |

## 3. 하위 프로젝트

| S | 계획 문서 | 상태 | 시작 | 완료 | 사용자 확인 |
|---|---|---|---|---|---|
| P0 | 00-roadmap §2 | 완료 | 2026-10-08 | 2026-10-08 | — |
| S0a | 02-s0a-spike | 대기 | | | 결과 보고 |
| S0b | 03-s0b-skeleton · 03b-s0b-app | 대기 | | | 6개 |
| S1 | 04-s1-agents-terminal · 04b-s1-app | 대기 | | | 10개 |
| S2 | 05-s2-runs | 대기 | | | 10개(전환점) |
| S3 | 06-s3-knowledge | 대기 | | | 10개 |
| S4 | 07-s4-s6(→ 07a) | 대기 | | | 8개 |
| S5 | 07-s4-s6(→ 07b) | 대기 | | | 6개 |
| S6 | 07-s4-s6(→ 07c) | 대기 | | | 7개 |

## 4. PR

PR 경계는 각 계획 문서의 "PR 묶음" 표다. 머지하면 PR 번호와 머지 커밋을 적는다.

| PR | 브랜치 | 과제 | 상태 | PR 번호 | 머지 커밋 |
|---|---|---|---|---|---|
| P0 | `docs/v1-plan` | 계획 폴더 반영 | push(PR 없음, PR A에 포함) | — | — |
| S0a | `docs/v1-plan` | S0a Task 8 결과·설계 반영(직접 커밋, PR 없음) | 대기 | — | — |
| A | `chore/rebuild/clean-slate` | S0b Task 1 | 대기 | | |
| B | `feat/daemon/protocol` | S0b Task 2–6 | 대기 | | |
| C | `feat/daemon/core` | S0b Task 7–15 | 대기 | | |
| D | `feat/app/skeleton` | S0b Task 16–21 | 대기 | | |
| E | `feat/app/windows-shell` | S0b Task 22–24 | 대기 | | |
| F | `feat/daemon/streams` | S1 Task 1–2 | 대기 | | |
| G | `feat/terminal/core` | S1 Task 3–4 | 대기 | | |
| H | `feat/git/agent-subset` | S1 Task 5–6 | 대기 | | |
| I | `feat/agents/core` | S1 Task 7–10 | 대기 | | |
| J | `feat/projects/launch-mcp` | S1 Task 11–12 | 대기 | | |
| K | `feat/app/terminal` | S1 Task 13–16 | 대기 | | |
| L | `feat/app/agents` | S1 Task 17–19 | 대기 | | |
| M | `feat/app/projects-settings` | S1 Task 20–21 | 대기 | | |
| N | `feat/app/windows-integration` | S1 Task 22–23 | 대기 | | |
| S1 마무리 | `chore/s1-wrap` | S1 Task 24 | 대기 | | |
| S2 상세화 | `docs/s2-detail` | S2 Task 1 | 대기 | | |
| O | `feat/runtime/core` | S2 Task 2–6 | 대기 | | |
| P | `feat/runtime/observe` | S2 Task 7–10 | 대기 | | |
| Q | `feat/secrets` | S2 Task 11–12 | 대기 | | |
| R | `feat/app/runs` | S2 Task 13–15 | 대기 | | |
| S2 마무리 | `chore/s2-wrap` | S2 Task 16 | 대기 | | |
| S3 상세화 | `docs/s3-detail` | S3 Task 1 | 대기 | | |
| S | `feat/documents/core` | S3 Task 2–3 | 대기 | | |
| T | `feat/notes/core` | S3 Task 4–6 | 대기 | | |
| U | `feat/search/core` | S3 Task 7 | 대기 | | |
| V | `feat/activity/core` | S3 Task 8–9 | 대기 | | |
| W | `feat/app/notes` | S3 Task 10–11 | 대기 | | |
| S3 마무리 | `chore/s3-wrap` | S3 Task 12 | 대기 | | |
| S4 상세화 | `docs/s4-detail` | S4-1 | 대기 | | |
| X | `feat/editor/core` | S4-2–S4-4 | 대기 | | |
| Y | `feat/git/full` | S4-5–S4-7 | 대기 | | |
| Z | `feat/app/source` | S4-8–S4-9 | 대기 | | |
| S5 상세화 | `docs/s5-detail` | S5-1 | 대기 | | |
| AA | `feat/api/http` | S5-2–S5-3 | 대기 | | |
| AB | `feat/api/streams` | S5-4 | 대기 | | |
| AC | `feat/api/grpc-mcp` | S5-5 | 대기 | | |
| AD | `feat/webhooks` | S5-6 | 대기 | | |
| AE | `feat/tools/transforms-doctor` | S5-7–S5-8 | 대기 | | |
| S5 마무리 | `chore/s5-wrap` | S5-9 | 대기 | | |
| S6 상세화 | `docs/s6-detail` | S6-1 | 대기 | | |
| AF | `feat/release/workflow` | S6-2 | 대기 | | |
| AG | `feat/app/update` | S6-3 | 대기 | | |
| AH | `chore/release-v1` | S6-4–S6-6 | 대기 | | |

## 5. 계획과 달라진 점

실행 중 계획과 다르게 한 것을 적는다(컴파일 오류 수정, 이름 변경, 순서 변경 등). 다음 상세화 과제가 이 표를 읽는다.

| 날짜 | 위치(문서·과제) | 계획 | 실제 | 이유 |
|---|---|---|---|---|

## 6. 세션 기록

세션이 끝날 때 한 줄을 덧붙인다. 오래된 줄은 지우지 않는다.

| 날짜 | 하네스 | 한 일 | 남긴 상태(브랜치·커밋·다음 단계) |
|---|---|---|---|
| 2026-10-08 | Claude Code | 세 계획(rebuild·CV·AR)을 이 폴더로 통합·검수(08 §8·§9), 원본 세 폴더 삭제, P0 | `docs/v1-plan`에 커밋·push(PR·CI 없음). 원본 체크아웃의 추적 안 된 사본은 지움. 다음: S0a |
