# Devbox v1 최종 계획 — 로드맵

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement each plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

- 상태: **최종 계획 확정(2026-10-08)** · 구현 미착수 · 진행 상태는 [PROGRESS.md](PROGRESS.md)
- 작성: 2026-10-07 재구축 계획 → 2026-10-08 다른 두 검토 계획과 통합·검수 · 기준 원격 `main@72ff50c7`(v0.9.0 공개본). 이 PC의 로컬 main에는 원격에 없는 커밋 2개(`b2a508b6` 의존성 갱신, `1b84aa9d` 완료 계획 정리)가 더 있다(PROGRESS §2)
- 이 폴더가 devbox 개선의 **유일한 계획**이다. 통합 전의 세 계획(재구축 계획, v0.9.0 리뷰 교차 검증 CV, Astra 교차 검증 AR)은 내용을 이 폴더에 모두 옮긴 뒤 지웠다(08 §8·§9).

**Goal:** v0.9.0(Tauri 4제품 + Windows 데몬 + WSL helper)을 "WSL Linux 데몬 + Windows의 얇은 Tauri 앱" 하나로 다시 지어, 에이전트·실행·터미널 중심의 개인 개발 허브를 높은 품질과 쉬운 사용성으로 제공한다.

**Architecture:** WSL 안의 `devbox daemon`(Rust, systemd user 서비스)이 모든 도메인 상태·데이터·프로세스를 소유한다. Windows의 `Devbox.exe`(Tauri v2)는 화면·트레이·단축키·알림·활동 수집만 맡고, `wsl.exe` stdio 브리지로 데몬과 프레임을 주고받는다. 화면은 React 한 벌이고, 요청·응답·오류 타입은 Rust 선언에서 생성한다.

**Tech Stack:** Rust 1.98(tokio, serde, ts-rs 12, rusqlite, portable-pty, mimalloc), Tauri v2.12, React 19, TypeScript 5.8, Vite 7, TanStack Router·Query, Zustand, Radix Primitives, CodeMirror 6, xterm.js 6, pnpm 9, Biome, Vitest, Playwright, systemd user, tmux ≥ 3.2.

**Spec:** [01-design.md](01-design.md) (설계 원장) · [08-review-record.md](08-review-record.md) (검토 기록)

## 0. 처음 읽는 사람에게

이 계획은 여러 세션(Claude Code·Codex, 다른 날, 다른 PC)이 이어서 실행한다. 어느 세션이든 아래 순서로 시작한다.

1. [PROGRESS.md](PROGRESS.md) — 지금 어디까지 왔고 다음에 무엇을 하는지.
2. 이 문서 §3 — 실행 규칙, 특히 §3.1 인계 절차와 §3.2 먼저 물을 것.
3. 지금 하위 프로젝트의 계획 문서(§1 표) — 그 문서 머리의 **Spec** 줄이 가리키는 01-design 절을 함께 읽는다.
4. 계획에 없는 판단이 필요할 때만 01-design 전체와 08(왜 이렇게 정했는지)을 읽는다.

**30초 요약.** v0.9.0(Tauri 네 제품 + Windows 백그라운드 agent + WSL helper)을 "WSL 안의 Rust 데몬 하나 + Windows의 얇은 Tauri 앱 하나"로 다시 짓는다. 사용 빈도 순서(에이전트 ≫ 실행 = 터미널 ≫ 기록 > 편집·API·변환)대로 S0a(구조 실험) → S0b(뼈대) → S1(에이전트·터미널) → S2(실행·관찰, 여기서 일상 사용을 새 빌드로 옮김) → S3 → S4 → S5 → S6(v1.0.0 공개)로 간다. 버전은 S6에서 한 번만 올리고, 그 전까지는 v0.9.0과 개발 빌드를 함께 쓴다.

**용어.** `S0a`…`S6` 하위 프로젝트, `Task N` 그 계획 안의 과제, `A`…`AH` 작업 묶음(브랜치 안의 중간 지점), `SC1`–`SC10` 성공 기준(01 §1), `D1`–`D16` 결정(01 §0), `R1`–`R14` 위험(01 §14), `IR-n` 통합 검토 소견(08 §8), `CV-F*`·`AR-F*`·`AR-D01` v0.9.0 리뷰 결함(00 §7), `PL-`·`FS-`·`UX-`·`SE-`·`PD-` 설계 교차 검증 소견(08 §1–5).

## 1. 문서 구성

| 문서 | 내용 | 상태 |
|---|---|---|
| [PROGRESS](PROGRESS.md) | **진행 원장**: 현재 위치, 막힌 것, S·PR 상태, 계획과 달라진 점, 세션 기록 | 계속 갱신 |
| [01-design](01-design.md) | 공통 설계(결정·범위·구조·데몬·통신·Windows 앱·데이터·화면·도메인·배포·검증·보안·위험) | v3(통합 검토 반영) |
| [02-s0a-spike](02-s0a-spike.md) | S0a 구조 확인 실험 | 계획 |
| [03-s0b-skeleton](03-s0b-skeleton.md) | S0b 걷는 뼈대 1/2: 저장소 초기화·프로토콜·데몬 | 계획 |
| [03b-s0b-app](03b-s0b-app.md) | S0b 걷는 뼈대 2/2: 화면 골격·Windows 껍데기 | 계획 |
| [04-s1-agents-terminal](04-s1-agents-terminal.md) | S1 에이전트·터미널·프로젝트 1/2: 데몬(스트림·tmux·Git·에이전트·프로젝트 설정·MCP) | 계획 |
| [04b-s1-app](04b-s1-app.md) | S1 에이전트·터미널·프로젝트 2/2: 화면·Windows 통합(알림·트레이·전역 단축키) | 계획 |
| [05-s2-runs](05-s2-runs.md) | S2 실행·관찰 | 계획(시작 시 상세화) |
| [06-s3-knowledge](06-s3-knowledge.md) | S3 기록 | 계획(시작 시 상세화) |
| [07-s4-s6](07-s4-s6.md) | S4 편집·Git, S5 개발 도구, S6 마무리·출시. 각 S의 첫 과제가 `07a`·`07b`·`07c`로 상세화 | 계획(시작 시 상세화) |
| [08-review-record](08-review-record.md) | 설계 교차 검증 소견과 판정(§1–7), 세 계획 통합 검토(§8, IR-1–IR-14), 검수(§9) | 완료 |
| `09-s0a-results.md` | S0a 측정 결과·판정·ADR 0017 초안 | S0a Task 8이 만든다 |
| `07a`·`07b`·`07c` | S4·S5·S6 단계별 상세 계획 | 각 S의 첫 과제가 만든다 |

**상세화 수준**
- S0a·S0b·S1은 단계별 코드·명령까지 적었다. 과제 번호는 S0b(Task 1–24)와 S1(Task 1–24)이 각각 두 문서에 이어진다.
- S2–S6은 과제·파일·인터페이스·핵심 테스트까지 적었다. 각 하위 프로젝트의 첫 과제가 "실제 S0b·S1 코드에 맞춰 이 계획을 단계별로 상세화"다. 앞 단계의 실제 타입 이름이 정해지기 전에 코드를 미리 적으면 틀린 계획이 되기 때문이다.

## 2. 하위 프로젝트 순서

```
P0 계획 반영 ──► S0a 구조 확인 ──► S0b 걷는 뼈대 ──► S1 에이전트·터미널 ──► S2 실행·관찰 ─┬─► S3 기록 ──► S4 편집·Git ─┬─► S6 마무리·출시(v1.0.0)
                                                                (전환점: 이후 A·B·C는 새 빌드로) └─► S5 개발 도구 ──────────────┘
                                                                                                 (한 세션이 두 트랙을 동시에, §3.6)
```

| S | 시작 조건 | 완료 조건(요약, 상세는 각 계획) | 사용자(§2.2: 기다리는 확인은 S0 확인 하나) |
|---|---|---|---|
| P0 | 사용자가 이 계획을 승인 | 이 폴더가 `docs/v1-plan` 브랜치에 커밋·push됨(PR·CI 없음, 아래) | — |
| S0a | P0 완료 | R1·R2·R3·R5·R10 측정과 판정, ADR 0017 초안 | 결과 보고만(기다리지 않음). 불합격으로 설계를 바꿔야 하면 멈추고 묻는다 |
| S0b | S0a 합격(또는 대체 경로로 설계 갱신 완료) | 옛 코드·CI·지침 교체, 데몬·브리지·생성기·화면 골격, `system.ping`·`projects.list/add`가 브라우저·Tauri 양쪽에서 동작, CI 통과 | **S0 확인**: 설치 → 확인 8개 + S0a 결과 검토. 승인하면 끝까지 자동 |
| S1 | S0b 머지 | 에이전트 전 과정, 격자 보기, 터미널 유지·분리 창, 트레이·전역 단축키·알림, MCP 기본, SC3·SC4·SC10 | 확인 목록 10개(머지 뒤, 기다리지 않음) |
| S2 | S1 머지 | 실행·관찰 전부, 비밀, Windows 대상 실행 | 확인 목록 10개 · **전환점:** 일상 A·B·C를 새 빌드로(사용자가 원할 때) |
| S3 | S2 머지(트랙 1, S5와 동시) | 노트·일일·캡처·활동·검색 | 확인 목록 10개 |
| S4 | S3 머지(트랙 1, S5가 아직이면 동시) | 편집기·LSP·Git 전체 | 확인 목록 8개 |
| S5 | S2 머지(트랙 2, S3와 동시) | API·웹훅·변환 도구·환경 점검 | 확인 목록 6개 |
| S6 | S4·S5 모두 머지 | S3·S4↔S5 연결(S6-1b), 설치기·업데이트·릴리스, 성공 기준 측정, 문서 정리, v1.0.0 공개 | **v1.0.0 공개 직전에 한 번 묻는다**(확인 목록 9개는 참고) |

**P0: 계획을 저장소에 넣기(2026-10-08 완료).**
- 이 폴더를 브랜치 `docs/v1-plan`에 커밋하고 push했다. PR은 열지 않았다. 사용자 요청: 계획 추가만으로 CI를 돌리지 않는다.
  - main은 "PR 필수 + 관리자에게도 적용" 보호라 직접 커밋할 수 없다.
  - 지금 저장소의 CI(v0.9.0용)는 PR·수동 실행·주간 일정에서만 돈다. 브랜치 push만으로는 아무 워크플로도 돌지 않는다.
- **S0b 브랜치가 생기기 전까지 계획의 원본은 `docs/v1-plan` 브랜치다.** S0b가 시작되면 S0b 브랜치의 사본이 최신이고, S0b PR 머지 뒤에는 main이다. 읽고 고치는 곳은 worktree `/home/jihoon/projects/devbox-wt/docs-v1-plan`이다. 없으면 만든다:
  ```bash
  cd /home/jihoon/projects/devbox && git fetch origin docs/v1-plan
  git worktree add ../devbox-wt/docs-v1-plan docs/v1-plan
  ```
  PROGRESS 갱신과 S0a 결과는 이 브랜치에 바로 커밋하고 push한다(PR 없음).
- S0b 브랜치는 `origin/docs/v1-plan`에서 갈라진다(03 Task 1). 그래서 계획 폴더는 S0b PR과 함께 main에 들어가고, 계획만을 위한 CI 실행은 없다.
- S0b PR 뒤에는 이 폴더의 변경도 그때 진행 중인 하위 프로젝트 브랜치에 함께 넣는다. 새 CI는 `docs/`와 루트 Markdown만 바뀐 PR에서 무거운 job을 건너뛰고 필수 검사 `ci-ok`만 바로 통과시킨다(03 Task 1의 `changes` job).

### 2.2 사용자 확인은 S0 끝 한 번 (사용자 결정 2026-10-09)

- 에이전트가 사용자를 **기다리는** 확인은 **S0 확인 하나**다(S0b 끝, 03b Task 24 Step 4). 승인하면 S1부터 S6 끝까지 에이전트가 스스로 진행한다.
- S0 확인의 내용: S0b 설치 파일로 확인 6개, S0a에서 사람이 재야 하던 두 가지(앱을 켠 채 30분 유휴 뒤에도 WSL·데몬이 살아 있는지, `wsl --shutdown` 뒤 다시 붙는 시간), S0a 결과 보고서 검토.
- S0a는 혼자 진행한다. 합격이면 결과를 보고만 하고 S0b로 넘어간다.
- S1–S5: PR CI가 통과하면 바로 rebase 머지하고 다음으로 간다. 각 하위 프로젝트의 **확인 목록**은 머지 뒤 설치 파일 받는 법과 함께 사용자에게 보낸다. 사용자는 원할 때 써 본다. 문제를 알리면 PROGRESS §2에 적고, 그때 진행 중인 브랜치에서 고친다(`fix:` 커밋).
- 그래도 멈추고 묻는 곳:
  - S0a 불합격으로 설계를 바꿔야 할 때.
  - §3.2의 상황(설계·범위 변경, PC 설정 변경, 되돌릴 수 없는 일 등). 이때도 그 결정에 기대지 않는 다른 과제·트랙은 계속한다.
  - **v1.0.0 공개 직전**(S6-6). 공개 릴리스는 되돌리기 어려워 한 번 묻는다.
- 사용자 확인을 기다리는 시간이 일정에서 빠진다(§2.1의 작업일이 그대로 달력 일수에 가까워진다).

### 2.1 일정 추정과 측정

과제는 모두 약 110개다(S0a 8 · S0b 24 · S1 24 · S2 17 · S3 12 · S4 9 · S5 9 · S6 7). 아래는 **실측 전 추정**이다. 과제 하나에 구현 하위 에이전트 + 검토 하위 에이전트 + Rust 빌드·시험이 드는 시간을, 코드까지 적힌 과제(S0b·S1)는 약 1시간, 과제 수준만 적힌 과제(S2–S6, 상세화 포함)는 약 1.5–2시간으로 잡았다. 오차는 ±50%로 본다.

| S | 과제 | 에이전트 작업 시간(추정) | 사용자 할 일 |
|---|---|---|---|
| S0a | 8 | 4–6시간 | 없음(사람 측정은 S0 확인으로) |
| S0b | 24 | 20–28시간 | **S0 확인 8개(기다림, 30분 유휴 포함)** |
| S1 | 24 | 24–34시간 | 확인 목록 10개(기다리지 않음) |
| S2 | 17 | 22–32시간 | 확인 목록 10개, **전환점** |
| S3 | 12 | 16–24시간 | 확인 목록 10개 |
| S4 | 9 | 16–24시간 | 확인 목록 8개 |
| S5 | 9 | 18–28시간 | 확인 목록 6개 |
| S6 | 7 | 8–13시간 | **v1.0.0 공개 직전 확인(기다림)** |
| 합계 | 110 | **약 130–190시간**(두 트랙의 합) | 기다리는 확인 2번(S0, 공개 직전) |

- 한 세션으로 차례로 하면 하루 8시간 기준 약 16–24 작업일이다. S3·S5 동시 진행(아래)으로 가장 긴 경로는 약 15–21 작업일이다. 하루 4시간이면 그 두 배다. 사용자를 기다리는 것은 S0 확인 하나라(§2.2) 그 밖의 대기는 더해지지 않는다.
- 일상 사용(에이전트·실행·터미널)을 새 빌드로 옮기는 전환점(S2 끝)까지가 약 72–102시간이다.
- CI는 PR 7번이라 전체 기간에 몇 시간뿐이다. 처음 계획(PR 47개)에서는 PR 준비·CI 대기·머지·정리가 PR마다 20–30분씩, 모두 약 15–25시간이었다.
- **측정:** 묶음이 끝날 때 PROGRESS의 묶음 행에 시작·끝 날짜와 실제 작업 시간을 적는다. S0b가 끝나면 실측 속도로 이 표를 다시 계산해 사용자에게 보고한다.
- **S3·S5 동시 진행(사용자 결정 2026-10-08, §3.6).** S2가 끝나면 실행 중인 세션이 트랙 1(S3 → S4)과 트랙 2(S5)를 하위 에이전트로 동시에 진행하고, S6는 둘 다 머지된 뒤 한다.
  - 기다리는 길이(가장 긴 경로)가 S3+S4+S5(50–76시간)에서 max(S3+S4, S5) = 32–48시간으로 준다. 대신 준비(S2 Task 15b)·연결(S6-1b)·나중 쪽의 rebase에 5–7시간이 더 든다. **순수하게 줄어드는 시간은 약 13–21시간**이다. 하루 8시간 기준 약 2–3일, 하루 4시간 기준 약 1주다(앞서 말한 "1–1.5주"는 하루 4시간 기준이었다).
  - 에이전트 작업 시간의 합은 줄지 않는다(약 130–190시간). 두 트랙이 동시에 도는 동안 PC 자원(Rust 빌드 두 개)과 사용자 확인이 겹친다. 사용자가 세션을 더 띄울 일은 없다.
  - 가장 긴 경로 기준 전체: 약 115–165시간, 하루 8시간이면 약 15–21 작업일.

- 버전은 S6에서 한 번만 올린다(`1.0.0-dev` → `1.0.0`). 중간 공개 릴리스는 없다.
- 재구축 동안 v0.9.0 설치본은 그대로 쓴다. 전환점(S2 완료) 뒤에는 A·B·C(에이전트·실행·터미널)를 새 빌드로 옮긴다.

## 3. 실행 규칙

**하위 프로젝트 하나 = 브랜치 하나 = PR 하나 (2026-10-08 결정)**

PR은 모두 7개다(S0b·S1·S2·S3·S4·S5·S6). P0·S0a는 PR 없이 `docs/v1-plan`에 커밋한다. 처음 계획은 사용자 흐름마다 PR을 나눠 47개였는데, PR마다 worktree 만들기·CI 대기·머지·정리가 반복되어 그 비용이 개발을 잡아먹는다는 사용자 지적으로 바꿨다.

| S | 브랜치 | worktree | 갈라지는 곳 |
|---|---|---|---|
| S0b | `v1/s0b-skeleton` | `../devbox-wt/v1-s0b-skeleton` | `origin/docs/v1-plan`(계획 폴더를 함께 main에 넣기 위해) |
| S1 | `v1/s1-agents-terminal` | `../devbox-wt/v1-s1-agents-terminal` | `origin/main` |
| S2 | `v1/s2-runs` | `../devbox-wt/v1-s2-runs` | `origin/main` |
| S3 | `v1/s3-knowledge` | `../devbox-wt/v1-s3-knowledge` | `origin/main` |
| S4 | `v1/s4-editor-git` | `../devbox-wt/v1-s4-editor-git` | `origin/main` |
| S5 | `v1/s5-tools` | `../devbox-wt/v1-s5-tools` | `origin/main` |
| S6 | `v1/s6-release` | `../devbox-wt/v1-s6-release` | `origin/main` |

- 원본 체크아웃(`/home/jihoon/projects/devbox`)은 `main`으로 두고 직접 작업하지 않는다. 다른 세션이 동시에 일할 수 있기 때문이다(01-design R9).
- 하위 프로젝트의 모든 과제는 그 브랜치·worktree 하나에서 차례로 한다. 과제마다 구현·검토 하위 에이전트가 바뀌어도 작업 위치는 같다.
- **작업 묶음**(각 계획의 "작업 묶음" 표, 글자 A–AH)은 브랜치 안의 중간 지점이다. 묶음이 끝나면 `pnpm check`(그 시점에 있는 검사) → `PROGRESS.md` 묶음 행 갱신 커밋 → push 한다. PR을 열지 않으므로 CI는 돌지 않는다. 세션 인계와 되돌아갈 지점이 된다.
- 중간에 CI로 확인하고 싶을 때만 `gh workflow run ci.yml --ref <브랜치>`(선택). 기본은 하지 않는다.
- 사소한 수정(오타·계획 문서만)은 지금 하위 프로젝트 브랜치에 함께 넣는다. 따로 PR을 만들지 않는다.

**PR과 머지**
- 하위 프로젝트의 마지막 묶음이 끝나고 `pnpm check`가 통과하면 PR을 한 번 연다. 그 CI가 하위 프로젝트 전체를 한 번 검사하고, `windows.yml`이 직접 써 볼 설치 파일을 만든다. 사용자는 그 설치 파일로 확인 목록을 원할 때 본다(§2.2).
- 필수 CI가 통과하면 **rebase 머지**한다(`gh pr merge --rebase`). 사용자 확인을 기다리는 PR은 S0b 하나뿐이다(§2.2). 과제 커밋이 main에 그대로 남아 나중에 원인을 찾기 쉽다(main은 선형 기록 필수라 squash·rebase만 된다). 사람 리뷰는 필수가 아니다(개인 저장소, 승인 0개 설정).
- **필수 검사는 `ci.yml`의 집계 job `ci-ok` 하나다**(IR-1). job을 더하면 `ci-ok.needs`에도 더한다. `windows.yml`은 `paths` 필터로 일부 PR에서만 돌므로 필수 검사에 넣지 않는다. 보호 규칙 전환은 S0b PR을 머지하기 직전에 한 번 한다(03b Task 24).
- 문서만 바꾼 PR(`docs/`·루트 Markdown)이 생기면 `changes` job이 무거운 job을 건너뛰고 `ci-ok`만 통과시킨다. 계획 갱신 때문에 전체 CI를 돌리지 않는다(사용자 요청 2026-10-08).
- CI는 전체 기간에 PR 7번(각 15분 이내) + Windows 빌드(각 40분 이내)만 돈다.
- 커밋은 영어 Conventional Commits로 쓰고, 과제 단위로 한다.

**계획 문서의 코드 읽는 법**
- 코드 블록 첫 줄의 `// <경로>`·`# <경로>` 주석은 그 코드를 둘 파일 위치 표시다. 파일에 넣지 않는다. 특히 셸 스크립트는 `#!/usr/bin/env bash`가 첫 줄이어야 한다.
- `(요점)`·`(테스트 위)`가 붙은 블록은 파일 전체가 아니라 핵심 부분이다. 나머지는 같은 과제의 Interfaces와 시험이 정한다.
- 계획 코드는 컴파일해 본 것이 아니다. 같은 의도로 고쳐도 되고, 고친 것은 PROGRESS §5에 적는다(§3.5).

**검증 순서**
1. 과제를 구현할 때: 실패하는 테스트를 먼저 쓰고, 그 테스트와 바꾼 crate·화면의 테스트만 돌린다.
   - Rust: `cargo test -p <crate>`
   - 화면: `pnpm --filter app exec vitest run <파일>`
2. 작업 묶음이 끝날 때와 PR을 열기 전에 로컬 전체를 한 번 돌린다: `pnpm check`(fmt·clippy·test·biome·vitest·tsc·gen-ts --check·금지어·L3).
3. CI 실패는 원인을 고친 뒤 다시 push한다. 통과한 무관한 검사는 반복하지 않는다.

**상태 기록**
- [PROGRESS.md](PROGRESS.md)가 원장이다. 작업 묶음이 끝날 때 그 브랜치에서 자기 묶음 행(상태·날짜·작업 시간)과 "현재 위치"를 고쳐 커밋하고, PR을 열거나 머지할 때 PR 표의 행을 고친다. 브랜치가 머지되면 main의 원장이 맞아진다.
- 각 계획 문서 머리의 "상태" 줄은 하위 프로젝트를 시작·완료할 때만 고친다. 체크박스는 실행 보조이며 진행 판단의 근거로 쓰지 않는다.
- 계획과 다르게 구현한 것(컴파일 오류 수정, 이름·순서 변경)은 PROGRESS §5에 적는다. 다음 하위 프로젝트의 상세화 과제가 그 표를 읽고 계획을 맞춘다.

**자원**
- `CARGO_BUILD_JOBS=4`, 무거운 검증은 한 번에 하나만 돌린다.
- 전역 target-dir(`~/.cache/targets/devbox`)은 worktree 경로마다 incremental 결과물이 따로 쌓인다(2026-09에 590 GiB까지 누적). 각 S 완료 때와 `du -sh ~/.cache/targets/devbox`가 150 GiB를 넘을 때 `scripts/sweep.sh`(S0b)를 실행한다(IR-8). 실행 중인 cargo·rustc가 없을 때만 한다. WSL 안에서 지워도 Windows 드라이브 공간은 VHDX 압축 전에는 돌아오지 않는다. 압축(`wsl --shutdown` 필요) 시점은 사용자가 정한다.
- 포트: 개발 서버는 화면 1450·gateway 1451, E2E는 화면 1460·gateway 1461이다(IR-7). 사용자가 `pnpm dev`를 켜 둔 채로 E2E를 돌릴 수 있어야 한다.

**실행 방식(확정 2026-10-08)**
- 모든 계획은 `superpowers:subagent-driven-development`로 실행한다. 과제마다 새 하위 에이전트가 구현하고, 다른 하위 에이전트가 §3.4 기준으로 검토한 뒤 다음 과제로 넘어간다. 하위 프로젝트 끝에 브랜치 전체 검토를 한 번 한다.
- 하위 에이전트는 실행 하네스가 주는 것을 쓴다(Claude Code의 하위 에이전트, Codex의 하위 작업 등). 그 스킬이 없는 하네스에서는 같은 순서(과제마다 구현 → 별도 검토 → 다음 과제)를 직접 지킨다.
- S0b PR이 머지되기 전까지 main의 `AGENTS.md`·`CLAUDE.md`는 v0.9.0용이다(하위 에이전트 모델 지정, 무거운 검증 절차 등). P0·S0a·S0b 동안은 **이 문서의 규칙이 우선한다**(사용자 승인 2026-10-08). S0b의 Task 1이 새 `AGENTS.md`로 바꾼다.
- 이유: S0b·S1만 과제 48개이고 과제 사이가 계획에 적힌 인터페이스로 이어진다. 기초(프레임·스트림 흐름 제어·병합 안전장치)의 실수는 이후 모든 단계로 번진다.
- 각 계획 머리의 "executing-plans" 선택지는 쓰지 않는다.
- 하위 에이전트도 그 하위 프로젝트의 worktree에서 일한다.

**정리**
- 머지된 worktree는 제거 → `git worktree prune` → 로컬·원격 브랜치 삭제 순으로 정리한다.

**직접 써 보기(dogfood) 설치 파일**
- 기본: `bash scripts/dogfood.sh <브랜치>`(S0b Task 24). CI의 `windows.yml`이 만든 `devbox-setup` 산출물을 받아 Windows의 `Downloads\devbox-dogfood\`에 둔다. Windows 쪽 Rust·MSVC·pnpm이 없어도 된다(2026-10-08 이 PC에는 셋 다 없음).
- 선택: Windows 쪽 도구를 설치했다면 `scripts/win-build.ps1`(로컬 빌드), Tauri 껍데기를 빠르게 고칠 때 `scripts/win-dev.ps1`. 설치는 §3.3.

### 3.1 세션 인계 절차

작업 세션은 언제든 끊기고 다른 세션(다른 하네스·다른 날)이 이어받는다. 그래서 모든 세션은 시작과 끝에 같은 절차를 따른다.

**시작할 때**
1. 원본 체크아웃 확인: `cd /home/jihoon/projects/devbox && git fetch origin && git status --short && git log -1 --oneline`. main이고 깨끗해야 한다. 아니면 고치지 말고 사용자에게 보고한다(다른 세션이 쓰는 중일 수 있다).
   - **S0b PR 머지 전에는** main에 이 폴더가 없다. S0b가 시작되기 전이면 `docs/v1-plan`(worktree `../devbox-wt/docs-v1-plan`), 시작된 뒤면 `v1/s0b-skeleton`(worktree `../devbox-wt/v1-s0b-skeleton`)의 사본을 `git pull --ff-only` 뒤 읽는다(§2 P0).
2. 진행 중인 일 찾기:
   ```bash
   git worktree list
   gh pr list --state open --json number,headRefName,title,isDraft
   git ls-remote --heads origin 'feat/*' 'fix/*' 'chore/*' 'docs/*'
   ```
   PROGRESS의 "작업 중" PR과 대조한다. 이 PC에 worktree가 없는데 원격 브랜치가 있으면 `git worktree add ../devbox-wt/<이름> <브랜치>`로 받는다.
3. 진행 중인 브랜치가 있으면(S2 뒤에는 트랙 1·2의 두 브랜치일 수 있다, §3.6) 각 브랜치의 `PROGRESS.md`를 읽고, `git -C <worktree> log --oneline origin/main..HEAD`로 끝난 과제(과제당 커밋 하나)를, `git -C <worktree> status --short`로 하던 과제를 확인한다. 커밋되지 않은 변경이 있으면 그 과제의 "실패 확인" 단계 시험을 다시 돌려 어디까지 됐는지 보고 이어서 한다.
4. 커밋되지 않은 변경은 절대 버리지 않는다(`git checkout -- .`·`git reset --hard`·`git clean`·`git stash drop` 금지). 이어갈 수 없으면 `git diff > /tmp/<브랜치>-<날짜>.patch`로 보존하고 사용자에게 묻는다.
5. 지금 과제의 계획 문서와 Spec 절을 읽고 §3.3 사전 준비를 확인한다.

**끝날 때(또는 컨텍스트가 모자라 요약될 것 같을 때)**
1. 끝난 과제는 과제 커밋을 한다. 하던 과제는 `wip(<영역>): Task N step M까지`로 커밋한다. 다음 세션이 그 과제를 끝낼 때 `git commit --amend`로 과제 커밋으로 바꾸고 `git push --force-with-lease`로 올린다(rebase 머지라 커밋이 main에 남으므로 wip를 남기지 않는다).
2. 그 브랜치의 `PROGRESS.md`에서 "현재 위치"와 묶음 행을 고치고, 세션 기록에 한 줄을 더한다. 커밋한다.
3. `git push -u origin <브랜치>`로 원격에 올린다(PR은 하위 프로젝트 끝에 한 번 연다. push만으로는 CI가 돌지 않는다). 다른 PC·다른 세션이 이어받을 수 있게 하기 위해서다.
4. 사용자에게 한 일·남긴 상태·다음 단계를 짧게 보고한다.

### 3.2 먼저 사용자에게 물을 것

아래는 실행 세션이 혼자 정하지 않는다. 멈추고 묻고, PROGRESS §2에 적는다.

| 상황 | 이유 |
|---|---|
| S0a 합격 기준 미달, 또는 01-design의 구조·결정(D1–D16·Q1–Q4)을 바꿔야 함 | 설계 원장 변경 |
| 범위를 더하거나 빼야 함(§2.6 제외 항목 되살리기 포함) | 사용자 결정 사항 |
| SC1–SC10 미달을 계획 안의 수정으로 고칠 수 없음 | 방향 판단 |
| 사용자 PC 설정 변경: `.wslconfig`, `/etc`, linger 켜기, `sudo`·`apt`·`winget` 설치, Windows 도구 설치 | 사용자 환경 |
| 사용자의 기존 tmux·systemd unit·Docker·방화벽을 건드려야 함 | 금지(AGENTS) |
| 커밋되지 않은 작업 버리기, 머지 안 된 브랜치·worktree 강제 삭제 | 되돌릴 수 없음 |
| 보호 규칙 변경이 권한 오류(403), 관리자 우회 머지가 필요해 보임 | 우회 금지 |
| 버전·태그·릴리스(S6), v0.9.0 긴급 수정 | 공개 작업 |
| 계획 순서를 바꾸거나 작업 묶음·PR 경계를 크게 바꿔야 함 | 원장 변경 |

멈추는 동안에도 그 결정에 기대지 않는 다른 과제·트랙은 계속한다. SC8 규모 예산 초과는 단순화 후보를 PROGRESS §2에 적어 알리되 기다리지 않는다.

혼자 정해도 되는 것: 계획 코드의 컴파일 오류를 같은 의도로 고치기, 시험 이름·파일 분할, 라이브러리 패치 버전, 문구 다듬기, 사용자 홈에 까는 개발 도구(`cargo install`, 저장소의 `pnpm install`), 저장소 범위의 git 설정(`merge.lockfile.driver`·`rerere`). 고친 것은 PROGRESS §5와 PR 본문에 적는다.

### 3.3 사전 준비

2026-10-08 이 PC에서 확인한 상태다. 새 PC나 새 세션은 시작할 때 다시 확인한다.

| 항목 | 확인 명령 | 2026-10-08 | 없으면 |
|---|---|---|---|
| Rust 1.98.1 + musl 대상 | `rustup toolchain list`, `rustup target list --installed --toolchain 1.98.1` | 있음(기본은 1.97.1, 저장소의 `rust-toolchain.toml`이 1.98.1을 고름) | `rustup toolchain install 1.98.1 --target x86_64-unknown-linux-musl` |
| cargo-sweep | `command -v cargo-sweep` | **없음** | 에이전트가 `cargo install cargo-sweep`로 설치한다(사용자 홈의 cargo 도구라 묻지 않음, S0b `scripts/sweep.sh`가 씀) |
| Node 24·pnpm 9 | `node -v`, `pnpm -v` | 24.18·9.15.9 | 사용자에게 |
| gh 인증 | `gh auth status` | 됨 | 사용자에게 |
| git 병합 드라이버·rerere | `git config --get merge.lockfile.driver`, `git config --get rerere.enabled` | 없음 | `git config merge.lockfile.driver true && git config rerere.enabled true`(clone마다 한 번, S2 Task 15b 전에. §3.6) |
| 커밋 이메일 | `git config user.email` | 저장소 설정이 개인 Gmail이라 **push가 거절됨**(GitHub 이메일 비공개 설정) | 커밋할 때 `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit …`. 저장소 설정을 이 주소로 바꿀지는 사용자가 정한다(PROGRESS §2) |
| jq·actionlint | `command -v jq actionlint` | 있음 | `sudo apt install jq`(사용자), actionlint는 바이너리 |
| tmux ≥ 3.2·systemd | `tmux -V`, `ps -p 1 -o comm=` | 3.6·systemd(`systemctl --user`는 `degraded`, 정상으로 봄) | §3.2 |
| claude·codex | `claude --version`, `codex --version` | 2.1.288·0.161.0 | S0a hook 실험 전 사용자에게 |
| Windows 쪽 Rust(msvc)·VS Build Tools(C++)·pnpm | PowerShell `Get-Command cargo,pnpm` | **없음** | 선택. 기본 dogfood는 CI 산출물이라 필요 없다. 로컬 Windows 빌드를 원하면 사용자가 `winget install Rustlang.Rustup`, `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`, `npm i -g pnpm@9`를 실행한다 |

### 3.4 검토 기준 (과제 검토 하위 에이전트)

검토자는 과제 문서·바뀐 파일·시험 결과를 보고 다음을 확인한다. 하나라도 어기면 되돌려 보낸다.
1. 실패하는 시험을 먼저 썼고, 그 시험과 바꾼 crate·화면의 시험이 통과한다.
2. 이름·시그니처가 과제의 **Interfaces → Produces**와 같다(다르면 PROGRESS §5에 적었는가).
3. 계획 문서의 Global Constraints와 01-design §5.3(오류·`Result<T, String>` 허용 범위)·§8.6(정합성 규칙)을 지킨다.
4. 바꾼 도메인의 01-design §9.1 회귀 조건과 00 §7 추적표의 시험이 이 과제에 걸려 있으면 들어 있다.
5. 과제 범위를 넘는 기능·리팩터링·방어 장치가 없다. 파일 600줄·컴포넌트 300줄을 넘지 않는다.
6. 비밀·문서 내용·경로가 로그에 나가지 않는다. 사용자의 기존 tmux·unit·Docker를 건드리지 않는다.

### 3.5 막혔을 때

| 상황 | 할 일 |
|---|---|
| 계획 코드대로 컴파일·시험이 안 됨 | 같은 의도로 고친다 → PROGRESS §5. 의도 자체가 틀렸으면 §3.2 |
| 계획 문서의 이름과 이미 머지된 코드가 다름 | 코드가 사실이다. 다음 상세화 과제에서 계획을 고친다 |
| CI만 실패, 로컬은 통과 | 원인을 고친다. flaky면 그 PR에서 고치거나 지운다(재시도 래퍼 금지) |
| 시험이 사용자 환경(systemd user·tmux)에 따라 다름 | 시험은 `DEVBOX_INSTANCE=test-<난수>` 자원만 쓰는지 확인. CI에서 systemd user를 못 쓰면 01-design §11의 대체(그 시험만 WSL 로컬 필수) |
| 디스크 부족 | `du -sh ~/.cache/targets/devbox` → `scripts/sweep.sh`. 그래도 모자라면 사용자에게 |
| push가 `push declined due to email privacy restrictions`로 거절됨 | 그 브랜치의 내 커밋을 noreply 주소로 다시 쓴다: `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit --amend --no-edit --reset-author`(여러 개면 `git rebase -x '…같은 명령…' origin/main`). §3.3 |

### 3.6 S3·S5 동시 진행 (사용자 결정 2026-10-08)

**한 세션이 두 트랙을 동시에 진행한다 (사용자가 세션을 더 띄우지 않는다)**
- S2 PR이 머지되면, 그때 실행 중인 세션(이하 오케스트레이터)이 그대로 두 트랙을 동시에 진행한다. 사용자는 세션을 새로 띄우거나 프롬프트를 더 줄 필요가 없다.
  - 트랙 1: S3 → (S3 머지 뒤) S4. 브랜치 `v1/s3-knowledge` → `v1/s4-editor-git`, worktree `../devbox-wt/v1-s3-knowledge` → `../devbox-wt/v1-s4-editor-git`.
  - 트랙 2: S5. 브랜치 `v1/s5-tools`, worktree `../devbox-wt/v1-s5-tools`.
- 진행 방법(`superpowers:subagent-driven-development`를 두 트랙에 동시에 적용하고, 동시 실행은 `superpowers:dispatching-parallel-agents`를 따른다):
  1. 두 worktree를 만든다: `git worktree add ../devbox-wt/v1-s3-knowledge -b v1/s3-knowledge origin/main`, `git worktree add ../devbox-wt/v1-s5-tools -b v1/s5-tools origin/main`.
  2. 각 트랙의 다음 과제를 맡을 구현 하위 에이전트를 **백그라운드로 동시에** 띄운다. 프롬프트에는 그 과제 본문, 그 트랙의 worktree 경로, 트랙 환경 변수(아래 자원), "다른 트랙의 파일은 건드리지 말 것(아래 공유 파일 표)"을 넣는다.
  3. 한 트랙의 구현이 끝나면 그 트랙의 검토 하위 에이전트(§3.4)를 띄운다. 다른 트랙은 기다리지 않고 계속 간다. 검토가 통과하면 그 트랙의 다음 과제로 간다. 트랙 **안**에서는 과제가 차례로, 트랙 **사이**는 동시에 간다.
  4. 동시에 도는 하위 에이전트는 트랙당 하나(구현 또는 검토)로, 모두 둘을 넘지 않는다.
  5. 첫 과제인 상세화(S3 Task 1, S5-1)도 두 트랙에서 동시에 한다.
  6. 묶음 끝(검사·PROGRESS·push)과 PR·머지는 트랙마다 따로 한다. 확인 목록은 머지 뒤 어느 트랙 것인지 밝혀 사용자에게 보내고 기다리지 않는다(§2.2). §3.2 상황으로 한 트랙이 멈추면 다른 트랙은 계속 간다.
  7. 오케스트레이터는 하위 에이전트의 결과 요약만 받고, 트랙 상태는 그때그때 각 브랜치의 PROGRESS에 적는다. 컨텍스트가 요약되거나 세션이 끊겨도 §3.1로 두 트랙을 모두 찾아 이어받는다.
- 하네스가 하위 에이전트를 백그라운드로 동시에 돌리지 못하면, 오케스트레이터가 두 트랙의 과제를 번갈아 한다. 사용자가 할 일은 그대로 없고, 시간 단축만 없다.
- S6는 S4·S5가 모두 머지된 뒤 한 트랙으로 돌아간다.
- 두 쪽이 다 쓰는 바탕(`index.db` 도메인별 버전, CodeMirror `CodeView`·색 모듈, 잠금·생성 파일 병합 규칙)은 S2 Task 15b에서 미리 만든다. 두 쪽을 잇는 기능은 S6-1b다. 그래서 동시 진행 중에는 서로의 기능을 쓰지 않는다.

**자원**
- Rust target-dir: 트랙 1은 기본(`~/.cache/targets/devbox`), 트랙 2는 `CARGO_TARGET_DIR=$HOME/.cache/targets/devbox-s5`(트랙 2 하위 에이전트의 명령 앞에 붙인다). 같은 폴더를 쓰면 cargo의 빌드 잠금 때문에 서로 기다려 동시 진행의 이득이 사라진다. S5 머지 뒤 `devbox-s5` 폴더는 지운다(수십 GiB).
- 두 트랙 모두 `CARGO_BUILD_JOBS=3`(합 6). WSL 메모리(20 GB)가 모자라면 한쪽을 잠시 멈춘다. 이 PC의 wsl-resource-guard가 메모리 압박 때 세션을 끝낼 수 있다.
- 전체 검사(`scripts/check.sh`)와 E2E(`pnpm --filter app e2e`)는 `flock`으로 한 번에 하나만 돈다(S0b Task 21). 다른 트랙의 검사가 끝나기를 기다리는 것은 정상이다.
- 데몬 시험은 `DEVBOX_INSTANCE=test-<난수>`라 서로 겹치지 않는다.

**공유 파일**

| 파일 | 규칙 |
|---|---|
| `app/src/rpc/gen/rpc.ts` | 손으로 합치지 않는다. 충돌하면 병합 드라이버가 main 판을 남기고, rebase가 끝난 뒤 `pnpm gen`으로 다시 만든다 |
| `Cargo.lock`, `pnpm-lock.yaml` | 같은 방식. rebase 뒤 `cargo metadata --format-version 1 >/dev/null`(빠진 의존성만 더함)·`pnpm install`로 다시 만든다. CI가 `--locked`·`--frozen-lockfile`로 확인한다 |
| 데몬 등록(`crates/cli/src/daemon/routes.rs`·`state.rs`), 생성기 등록(`xtask/src/main.rs`), 화면 섹션·라우터(`app/src/shell/sections.tsx`·`router.tsx`), 오류 문구(`app/src/rpc/messages.ts`), workspace `Cargo.toml`·`app/package.json`의 의존성 목록 | 두 쪽이 줄을 더하기만 한다. 충돌하면 양쪽 줄을 모두 남긴다 |
| `crates/core`, `crates/protocol`, `app/src/ui`, `app/src/rpc`(gen 밖), `.github`, `scripts` | 동시 진행 중에는 고치지 않는다. 꼭 필요하면 PROGRESS §2에 적고, 먼저 머지할 쪽이 맡는다 |
| `PROGRESS.md` | 각 브랜치의 사본에서 자기 행만 고친다(트랙 1: S3·S4 행과 그 묶음, 트랙 2: S5 행과 그 묶음). "현재 위치"는 트랙별 줄을 쓴다. 충돌하면 양쪽을 모두 남긴다 |
| 계획 문서 | 자기 하위 프로젝트 부분만 고친다(트랙 1: 06, 07의 S4 절. 트랙 2: 07의 S5 절) |

**머지 순서(오케스트레이터가 한다)**
- 먼저 끝난 쪽은 평소대로 PR → CI → 머지한다.
- 나중 쪽은 PR을 열기 전에 main 위로 올린다.
  ```bash
  git fetch origin main
  git rebase origin/main   # 등록 파일 충돌은 양쪽 줄을 남기고 git add → git rebase --continue. 잠금·생성 파일은 드라이버가 처리
  cargo metadata --format-version 1 >/dev/null && pnpm install && pnpm gen
  git add -A && { git diff --cached --quiet || git commit -m "chore: refresh lockfiles and generated types after rebase"; }
  pnpm check && git push --force-with-lease origin <브랜치>
  ```
- 같은 등록 파일이 rebase 중 여러 번 충돌할 수 있으므로 `git config rerere.enabled true`를 켜 둔다(§3.3).
- S4는 S3 머지 뒤 `origin/main`에서 시작한다. 그때 S5가 아직이면 S4와 S5가 같은 규칙으로 동시에 간다.
- S3·S5의 확인 목록이 비슷한 때 사용자에게 갈 수 있다. 어느 쪽 것인지 밝혀 따로 보낸다.

## 4. v0.9.0 사용 중 회피 목록 (재구축 기간)

재구축 동안 v0.9.0을 계속 쓰므로, 확인된 결함을 피하는 방법을 둔다. v0.9.0은 패치하지 않는다(버전 규칙). 새 빌드로 옮기는 시점은 영역마다 다르다: 에이전트·실행·터미널은 S2 뒤, 노트·활동은 S3 뒤, 편집·Git은 S4 뒤, API·변환 도구는 S5 뒤.

| 결함 | 피하는 법 | 새 빌드로 옮기는 시점 |
|---|---|---|
| CV-F01 Git 충돌 미리보기 뒤 외부 편집을 오래된 해결안으로 덮어씀 | 충돌 해결은 VS Code나 터미널의 `git mergetool`로 한다 | S4 |
| AR-F01 Source의 push·pull 확인창이 그 사이 외부 checkout을 놓쳐 다른 브랜치를 push할 수 있음 | push·pull 확인 직전에 새로 고침을 누르거나, 터미널에서 한다 | S1(에이전트 병합·PR), S4 |
| AR-F02 서비스 health 검사: 주소 `localhost`는 항상 실패, "재시작 안 함" 서비스도 health 3회 실패면 다시 띄움 | health 주소는 `127.0.0.1`로 쓰고, 재시작을 원하지 않는 서비스는 health 검사를 끈다 | S2 |
| AR-F04 작업·서비스 저장 뒤 목록 조회 오류 | 저장을 다시 누르지 않는다(이미 만들어졌다). 목록만 새로 고친다 | S2 |
| CV-F03·AR-F13 Files 복구 목록에 옛 경로·이미 버린 초안이 다시 나옴 | 내용을 확인하고 그 항목만 폐기한다 | S4 |
| CV-F04 API 실행에서 이전 캡처 값 재사용 | 캡처를 쓰는 컬렉션은 실행 전에 세션 변수를 비운다 | S5 |
| CV-F05 잘못된 헤더가 전송 전에 걸러지지 않음 | 헤더 이름에 공백·특수문자를 넣지 않는다 | S5 |
| AR-F06·F07 API 본문 종류·gRPC 메서드를 바꾸면 쓰던 초안이 사라짐 | 종류·메서드를 바꾸기 전에 요청을 저장한다 | S5 |
| CV-F08 Activity 저장 오류 구간 손실 | 디스크 공간 부족 경고가 나면 활동 기록을 일시중지한다 | S3 |
| AR-D01 Notes에서 링크가 많은 노트의 이름 바꾸기 도중 종료하면 링크·대상이 어긋남 | 이름 바꾸기가 끝날 때까지 앱을 닫지 않는다 | S3 |
| CV-F09 업데이트 진행 중 화면 이동 시 제어 소실 | v0.9.0 업데이트 화면에서 다른 화면으로 이동하지 않는다(새 버전도 없음) | S6 |

**긴급 수정(계획하지 않음, IR-14).** 위 회피로 막을 수 없는 데이터 손상급 결함이 새로 나오면, `v0.9.0` 태그에서 수정 브랜치를 만들어 옛 릴리스 절차로 고치는 길은 남아 있다(옛 워크플로는 태그에 있다). 실제로 할지는 그때 사용자에게 묻는다.

## 5. 결정 (사용자, 2026-10-08 권장안 채택)

| ID | 질문 | 결정 |
|---|---|---|
| Q1 | 비밀 키를 DPAPI 봉인으로 둘지, WSL 0600 키 파일 + 복구 문자열로 단순화할지 | **DPAPI 봉인.** 데몬이 interop으로 직접 풀고, interop이 안 되는 환경에서만 Windows 앱이 대신 푼다. 복구 문자열·`key_id`로 분실에 대비한다(01-design §6.3) |
| Q2 | 제외 후보 X-1–X-10 중 되살릴 것 | **없음.** 모두 v1에서 뺀다(01-design §2.6) |
| Q3 | 앱 identifier `io.github.jihoon22lee.devbox` 사용 | **사용** |
| Q4 | 노트 vault 기본 위치 `~/notes` | **사용** |
| — | 실행 방식 | **Subagent-driven**(§3 실행 방식) |
| — | PR 단위 | **하위 프로젝트당 하나, 모두 7개**(§3, 2026-10-08) |
| — | 사용자 확인 | **S0 끝 한 번만 기다린다**(§2.2, 2026-10-09). 그 뒤는 v1.0.0 공개 직전과 §3.2 상황에서만 묻는다 |
| — | S3·S5 | **동시 진행**(§3.6, 2026-10-08). 한 세션이 하위 에이전트로 트랙 1(S3 → S4)과 트랙 2(S5)를 함께 돌린다. 사용자가 세션을 더 띄우지 않는다 |

**남은 확인**
- (해결 2026-10-08) 01-design §9.3의 "AU-K2"는 10-03 감사 K2 결함이다. 관찰이 반복된 뒤 idle로 끝나면 마지막 입력 뒤 298초까지 활동으로 집계했다. v0.9.0에 이미 고쳐져 있으므로 v0.9.0 코드와 테스트를 그대로 이식하면 된다([06 S3](06-s3-knowledge.md) Task 8). 출처: `docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md`(v0.9.0 공개본 커밋 `72ff50c7`에 있음: `git show 72ff50c7:<경로>`).

## 6. 실행 시작 프롬프트

새 세션에서 실행을 시작할 때 아래를 그대로 준다.

```
/home/jihoon/projects/devbox/docs/superpowers/plans/2026-10-08-devbox-v1/ 의 devbox v1 계획을 이어서 실행해.
00-roadmap.md §0의 순서대로 PROGRESS.md → 00-roadmap.md §3 → 지금 하위 프로젝트 계획을 읽고,
§3.1 인계 절차로 진행 중인 브랜치·worktree를 먼저 찾아 이어받아.
원본 체크아웃은 main 으로 두고, 하위 프로젝트마다 §3 표의 브랜치·worktree 하나를 써. 작업 묶음이 끝나면 push만 하고, PR은 하위 프로젝트 끝에 한 번 열어.
superpowers:subagent-driven-development 로 과제마다 구현 하위 에이전트와 검토 하위 에이전트(§3.4 기준)를 돌려.
§3.2에 해당하면 그 일만 멈추고 물어봐. 사용자를 기다리는 확인은 S0b 끝의 S0 확인 하나다. 그 뒤로는 §2.2대로 v1.0.0 공개 직전까지 멈추지 말고 진행해. 세션을 끝내기 전에 §3.1 "끝날 때"를 꼭 해.
S2 이후 계획의 첫 과제는 상세화다. S2가 끝나면 §3.6대로 이 세션이 트랙 1(S3→S4)과 트랙 2(S5)를 하위 에이전트로 동시에 진행해. 세션을 따로 띄우지 않는다. 00-roadmap §7 추적표와 PROGRESS §5를 반영해 먼저 커밋하고 진행해.
```

## 7. 검토 결함 추적표 (CV·AR → 새 구조)

CV·AR이 확정한 v0.9.0 결함을 새 구조에서 어떻게 막는지 적는다(08 §8). "구조로 제거"도 시험 하나는 둔다. 구조가 막는다는 주장을 시험으로 고정하기 위해서다. 입력값은 AR이 실행으로 재현한 값이다.

| 출처 | v0.9.0 결함 | 새 구조에서 | 설계 | 시험 위치와 입력 |
|---|---|---|---|---|
| CV-F01 | Git 충돌 미리보기 뒤 외부 편집을 오래된 해결안으로 덮어씀 | 회귀 시험 | 01 §9.1-1 | S4-6: 미리보기 → 외부 수정 → 적용 → 적용 거부, 작성 중 해결안 보존, stage 변화 0 |
| CV-F02 | 복구 CAS 오류 코드를 문자열 정규식으로 분류해 재시도가 반복됨 | 구조로 제거: 오류는 생성된 코드 타입으로만 분류 | 01 §5.3, §9.1-2 | S3 Task 3: `documents.conflict`를 두 번 연속 받아도 같은 문서 버퍼·저널을 덮지 않음 |
| CV-F03·AR-F13 | dirty 파일 rename·다시 읽기·디스크 것 선택 뒤 옛 복구본이 다음 시작에 다시 나옴 | 회귀 시험 | 01 §8.4 표(rename·Conflict·Recovering 행) | S3 Task 2·3: dirty → rename → 저장 → 다시 열기에서 옛 경로 저널 0개. Conflict → [디스크 것] → 다시 열기에서 Recovering 아님 |
| CV-F04·AR-F03 | 건너뛰기·취소된 생산 요청의 이전 캡처 값을 다음 요청이 사용 | 회귀 시험 | 01 §9.1-4 | S5-3: 이전 토큰이 있는 세션에서 생산 요청을 비밀 재검토 skip·파일 누락 skip·취소 → 소비 요청 송신 0회, `stopOnFailure` 참·거짓 모두. 정상 토큰 갱신은 이전 값을 입력으로 쓸 수 있음 |
| CV-F05 | 문법이 틀린 활성 헤더를 조용히 빼고 전송 | 회귀 시험 | 01 §9.1-4 | S5-2: 이름 `X API Key`, 값에 개행 → 전송 전 `api.invalid_header{index}`, 네트워크 송신 0회 |
| CV-F06 | 내부 복구 파일이 노트 색인·위키링크·rename 재작성 대상 | 구조로 제거: 저널은 데이터 폴더. 같은 폴더 임시 파일·`.trash/`는 제외 규칙 | 01 §9 `documents`, §9.1-2 | S3 Task 4·7: `.devbox-tmp-*`·`.trash/` 아래 노트가 트리·검색·백링크·링크 재작성에 나오지 않음 |
| CV-F07·AR-F11 | 내용 변화 없는 WSL heartbeat가 검색을 다시 실행하고 선택·메뉴를 닫음 | 구조로 제거: 관찰 시각만 바뀐 상태는 rev를 올리지 않음 | 01 §5.5 R8, §9.1-5 | S3 Task 7: 색인 상태의 관찰 시각만 바뀐 이벤트 3회 → `search.query` 추가 호출 0, 선택 유지 |
| CV-F08·AR-F05 | 활동 저장 실패 시 확정 세션 유실, 화면은 정상 수집 표시 | 회귀 시험 | 01 §9.1-6 | S3 Task 8: 3세션 중 두 번째 INSERT 실패 주입 → "저장 중단" 표시 → 회복 뒤 3건 모두, 중복 0 |
| CV-F09·AR-F16 | 업데이트 다운로드 중 화면 이동 뒤 진행·취소를 잃음 | 구조로 제거: 진행은 Windows 앱이 소유 | 01 §9.1-7 | S6-3: 다운로드 중 창 닫기·다시 열기 → 같은 진행·취소 |
| CV-F10 | 위키링크 rebuild helper | 해당 없음(CV가 결함 아님으로 판정) | — | — |
| AR-F01 | 승인 뒤 외부 checkout이 일어나면 승인하지 않은 브랜치를 push | 회귀 시험 | 01 §5.2 `expect`, §9.1-12 | S1 Task 6·9(병합·PR·폐기), S4-5(push·pull): A 상태 확인 → 외부에서 B checkout → 승인 → `git.state_changed`, 원격 ref 변화 0. 변화 없는 A는 정상 |
| AR-F02a·b·c | `localhost` health 실패, "재시작 안 함"인데 health 실패로 재시작, 시작 유예 미적용 | 구조로 제거: health로 재시작하지 않음 | 01 §9.1-17 | S2 Task 4·8: `restart = "no"` 서비스가 `exit 1` → 재시작 0회. `127.0.0.1`·`::1`·`0.0.0.0`에서 수신하면 모두 "준비됨" |
| AR-F04·AR-F15 | 생성 성공 뒤 목록 조회 실패 → 다시 저장하면 중복 생성. 서비스 상세가 처음 읽은 값에 고정 | 구조로 제거: `clientRequestId` 멱등, 생성 응답으로 화면 확정, rev 규칙 | 01 §9.1-13 | S2 Task 13: 생성 성공 + `runtime.list` 실패 주입 → 편집기는 생성된 ID의 수정 모드, 다시 저장해도 생성 1회. 시작·재시작 뒤 열린 상세의 PID·실행 ID가 최신 |
| AR-F06·F07·F08 | 본문 종류 전환·gRPC 필터 전환으로 초안 소실, 늦은 gRPC 응답이 다른 메서드 화면 아래 출처 없이 표시 | 회귀 시험 | 01 §9.1-14 | S5-3: GraphQL → 없음 → GraphQL에서 세 필드 보존, JSON → GraphQL 편집 → JSON에서 본문 보존(송신·저장은 활성 종류만). S5-5: 메서드 A 초안 → 필터로 B → A에서 초안 보존. A 대기 중 B 선택 → A 응답은 "A의 결과"로 표시 |
| AR-F09 | 정규식 위치를 Rust byte로 주고 화면은 UTF-16으로 자름 | 구조로 제거: 변환 도구는 TS에서 실행 | 01 §9.1-15 | S5-7: `가a나`에서 `a` → 1..2, `😀a`에서 `a` → 2..3, 조각을 이으면 원문과 같음 |
| AR-F10 | 기본 템플릿의 빈 title 때문에 새 노트가 파일 이름으로 검색되지 않음 | 회귀 시험 | 01 §9.1-16 | S3 Task 4·7: `title: ""`·`title: "  "`·title 없음 → 색인 제목은 파일 이름(`idea`), 이름 검색 1건 |
| AR-F12 | 늦게 끝난 Timeline 조회가 최신 수집 제어 표시를 덮음 | 구조로 제거: 조회마다 별도 query + R5 | 01 §5.5, §9.1-5 | S3 Task 11: `activity.day` 응답을 늦게 풀어도 `activity.status`의 "수집 중" 표시 유지 |
| AR-F14 | 빠른 반복 종료에도 재시작 간격이 1초로 고정 | 정책 결정: systemd 단계 증가 | 01 §4.3, §9.1-10 | S2 Task 4: `unit_args`에 `RestartSteps=5`·`RestartMaxDelaySec=30s` 포함, 한도 초과 → `failed` 알림 1회, 수동 재시작은 한도 초기화 |
| AR-D01 | 여러 노트 링크 재작성 도중 프로세스 종료 시 참조·대상 불일치 | 회귀 시험 | 01 §9.1-3 | S3 Task 5: 대상 이동 직후·첫 참조 재작성 뒤 두 지점에서 서비스 종료 → 다음 시작에 진행 저널로 나머지를 처리하거나 목록으로 보여 줌 |
