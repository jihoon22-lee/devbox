# Workbench 설계 — Project Workspace Orchestrator

- 상태: 제안(Proposal) — Stage 4
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §15.2, §10.2, §10.3, §17.8
- 선행: PR 2(identifier), PR 6(catalog), PR 14(crates/wsl), PR 28(ProjectProfile 설계), PR 26(producer snapshot), PR 31(service 상태 API)

## 1. 제품 정의

기존 앱 UI를 한 창에 복제하는 통합 앱이 아니다. 프로젝트를 기준으로 여러 앱과
서비스를 조정하고 상태를 요약하는 orchestration shell이다.

## 2. 핵심 흐름

```text
프로젝트 선택
  → Git/WSL/포트/서비스 사전 점검
  → Run Manager 서비스 시작
  → 예상 포트 준비 확인
  → WSL Desktop layout 열기
  → Code Pad workspace 열기
  → 필요하면 API request 열기
```

## 3. 앱 간 ownership

| 데이터 | owner(writer) | reader |
|---|---|---|
| ProjectProfile | **Workbench** (단일 writer) | life-log(attribution), wsl-desktop, code-pad, run-manager, port-manager |
| integration snapshot | producer 앱 | consumer 앱 (Workbench 포함) |
| 각 앱의 DB | 해당 앱 | **아무도 직접 수정하지 않는다** |

- Workbench는 다른 앱의 DB를 직접 수정하지 않는다.
- 실행 context는 §10.3의 명시적 CLI argument로 전달한다 (custom URL scheme은 나중).
- 다른 앱과의 상태 동기화는 integration snapshot 계약(§10.1) 또는 앱별 read API를 통해.

## 4. Start Workspace — 단계와 실패 정책

각 단계는 idempotency key로 실행 기록을 남긴다 (같은 실행을 두 번 눌러도 중복 시작 없음).

| # | 단계 | 성공 기준 | 실패 시 |
|---|---|---|---|
| 1 | 사전 점검 (Git/WSL/포트/서비스) | 모든 필수 점검 통과 또는 경고 표시 | 경고는 계속 진행, 치명 실패는 중단 |
| 2 | Run Manager 서비스 시작 | 서비스가 running | 해당 단계 실패 표시, 이후 단계 진행 |
| 3 | 예상 포트 준비 확인 | 포트 open 또는 retry 대기 | 대기(2초 × 5) 후 실패 표시 |
| 4 | WSL Desktop layout 열기 | 프로세스 시작 | 계속 (비차단) |
| 5 | Code Pad workspace 열기 | 프로세스 시작 | 계속 (비차단) |
| 6 | API request 열기 (선택) | 프로세스 시작 | 계속 (비차단) |

- "이미 실행 중이던 자원"과 "Workbench가 시작한 자원"을 구분한다:
  - 사전 점검에서 이미 running이던 서비스/포트 → Workbench가 종료하지 않는다.
  - Workbench가 시작한 것만 `Stop What I Started`의 대상.
- 부분 실패 후 상태를 사용자가 이해할 수 있어야 한다 (단계별 결과 + rollback 가능 여부 표시).

## 5. 실행 기록과 idempotency

- 실행마다 `workspace_run` 레코드: { run_id, profile_id, started_at, steps: [{name, status, started_owned}] }
- idempotency key: profile_id + 시작 시각(초 단위). 같은 초에 중복 실행 요청은 무시.
- `Stop What I Started`: run 기록에서 `started_owned=true`인 자원만 정리.

## 6. ProjectProfile 저장

- 저장: `%LOCALAPPDATA%\com.devbox.workbench\project-profiles.json` (임시 파일+rename 원자 교체)
- canonical identity: `crates/wsl::canonical_project_key` 단일 규칙
- 기존 두 저장소 흡수 (PR 28 §6): wsl-desktop localStorage `wsld-projects`, life-log settings `projects`

## 7. MVP 범위

- ProjectProfile CRUD + 기존 두 저장소 흡수
- read-only project health (Git, WSL distro, expected port, Run Manager service)
- wsl-desktop `gitStatus` 이관
- 앱 실행 context 전달 (§10.3 CLI argument)
- Start Workspace / Stop What I Started
- 단계별 실패·rollback 표시

## 8. 안전 경계

- 시작 전부터 실행 중이던 process/service는 자동 종료하지 않는다.
- 각 단계에 idempotency key 또는 실행 기록을 둔다.
- 다른 앱의 DB를 직접 수정하지 않는다.
- 앱이 없으면 Devbox Manager의 해당 앱 설치 화면으로 안내한다.
- 범용 앱·파일·웹 launcher를 만들지 않는다.

## 9. 완료 조건

- 시작 전부터 실행 중이던 process/service를 자동 종료하지 않는다
- 다른 앱의 DB를 직접 수정하지 않는다
- 앱이 없으면 Devbox Manager 설치 화면으로 안내한다
- 부분 시작 실패 후 상태를 사용자가 이해할 수 있다
- wsl-desktop에 프로젝트 목록·git 상태 코드가 남아 있지 않다
