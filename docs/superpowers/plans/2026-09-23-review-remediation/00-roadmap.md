# 리뷰 후속 작업 로드맵 (2026-09-23)

> 이 폴더의 정본이다. 실행 세션(Claude Code 또는 Codex, Native 방식)은 이 파일을 끝까지 읽은 뒤 §6 순서대로 **묶음(bundle) PR**을 하나씩 끝까지 실행한다. 계획 파일은 작업 단위이고 PR은 묶음 단위다(계획 43개 → PR 12개 + 릴리스 1개, D30). **버전은 모든 작업이 끝난 뒤 P3-01에서 한 번만 올린다(0.8.1 → 0.9.0, D29).** 시작 프롬프트는 §7에 있다.

## 0. 폴더 구성과 읽는 법

| 파일 | 내용 |
|---|---|
| `review.md` | 원본 리뷰. 결함 B1–B8, 보안 S1–S5, 성능 P1–P5, 구조 A1–A8, UX(§7), 제품 구성·신규 기능(§8) |
| `00-roadmap.md` | 이 파일. 결정(§2), 전역 제약(§3), 묶음 실행 규칙과 PR 공통 절차(§4), Windows acceptance 추적과 최종 게이트(§5), 묶음·계획 색인(§6), 시작 프롬프트(§7) |
| `p0-*.md` | Phase 0: 결함·고위험 수정, 계획 8개 |
| `p1-*.md` | Phase 1: 구조 개선, 계획 19개(P1-14는 PR A·B 두 부분) |
| `p2-*.md` | Phase 2: 백그라운드 서비스와 새 기능, 계획 12개(P2-05·P2-10·P2-12는 PR A·B 두 부분) |
| `p3-01-release-v0.9.0.md` | 유일한 릴리스(버전 올림·후보·사용자 필수 점검·태그·공개) |

- 계획 파일(PR A·B로 나뉜 것은 각 부분)이 작업 단위이고, §6의 묶음 하나가 PR 하나다. 합계: 묶음 PR 12개 + 릴리스 PR 1개(+ 공개 뒤 근거 기록 PR 1개).
- 모든 계획 파일은 같은 구성이다: 머리말(실행 방법) → Goal·Architecture·Tech Stack·Spec → Global Constraints → Review Focus(테스트가 직접 잡지 못하는 위험 5개와, 그것을 고정하는 과제 표시) → Branch·PR(속한 묶음) → Tasks(실패하는 테스트 → 실패 확인 → 구현 → 통과 확인 → 커밋) → 마지막 과제 "§4.4–§4.9 + Windows 실기 확인"(묶음의 마지막 계획에서만, §4.0).
- 파일 경로·함수 이름은 작성 시점(main `f28f08a4`, v0.8.1) 기준이다. 앞 계획이 옮기거나 이름을 바꾼 것은 그 계획서에 새 위치가 적혀 있다(예: P1-07 crate alias, P1-10 `crates/wsl-helper`, P1-14 `ipc/*.rs`, P2-02 `crates/workspace-core`). 계획의 코드와 실제 코드가 다르면 §4.3의 규칙을 따른다.
- 계획 파일의 체크박스는 고치지 않는다. 진행 상태의 정본은 ledger 이슈(§6 첫 줄)와 PR이다.

## 1. 목표와 범위

| 단계 | 묶음 | 내용 |
|---|---|---|
| Phase 0 | B1–B2 | 확인된 결함과 고위험 수정: 활동 개인정보 규칙(B1), 업데이트 캐시·제품 연결·단축키 화면·조기 오류 문구(B2–B5), Knowledge 오류 코드와 자동 저장(B6, D12), OneDrive·Dev Drive(B7), 웹훅 전송 형식(B8), 운영 로그·panic 기록(A7), describe 1회화(P1) |
| Phase 1 | B3–B8 | 구조: 기록 보관소와 ADR(A8), v0.7 잔재 제거(A3, D9), 의존성 통일·이름·포맷(A5·P4), 공용 crate(A4: process-tree·DPAPI·WSL helper·Markdown), 타입 IPC와 오류 코드(A1·A2), 프런트 공통 hook·분리(A5·P2), 터미널 push·유휴 루프 제거(P2·P3), API Studio native 저장소(A6), 되돌리기·진단 복사(UX), devbox-agent 설계(§8) |
| Phase 2 | B9–B12 | devbox-agent(런타임·웹훅·수집기를 백그라운드로, 트레이 하나, 로그인 자동 시작), Agent Hub, Devbox MCP 서버, Git 보강(branch·stash·hunk·amend·blame·충돌·gh PR), API Studio 보강(가져오기·파일 컬렉션·검증·러너·OAuth 2.0·TLS·코드 생성) |
| Phase 3 | R | **v0.9.0 릴리스 한 번**: 버전 0.8.1 → 0.9.0, 후보, 태그 전 사용자 필수 점검, 공개, 공개 뒤 업데이터 점검 |

- 중간 릴리스·태그·버전 변경은 없다.
- CI 대기를 줄이는 방법: PR을 묶음 12개로 줄이고, 머지는 필수 CI(5–20분)만 기다린다. Windows 전체 acceptance(60–90분)는 머지를 막지 않고 뒤따라 확인한다(§4.7, §5).
- 범위 밖은 §2.4 백로그에 있다.

## 2. 결정

### 2.1 사실 확인(Q1–Q3)

| | 질문 | 답 | 반영 |
|---|---|---|---|
| Q1 | 두 PC의 v0.7 데이터를 v0.8로 이전했나 | 두 PC 모두 기존 데이터를 지우고 v0.8로 새로 구축한다 | v0.7 가져오기 제거(D9, P1-02–P1-05) |
| Q2 | 회사 PC의 OneDrive·Dev Drive | 회사 PC는 보안상 OneDrive 같은 클라우드를 쓸 수 없다 | OneDrive 실기는 집 PC만(P0-05) |
| Q3 | 회사 PC에서 서명 없는 실행 파일 | 가능(이전 버전 설치 경험) | 서명 없이 배포 유지(§2.3) |

- 두 PC 모두 Windows 11, WSL은 집 Ubuntu 26.04, 회사 Ubuntu 24.04다. 지원 OS는 Windows 11 전용이다. 현재 공개 버전은 v0.8.1(2026-09-21)이다.

### 2.2 결정(D1–D30)

| D | 주제 | 결정 | 이 계획에서 |
|---|---|---|---|
| D1 | 계획서 구성 | (b) 모든 단계의 상세 계획을 지금 작성 | 이 폴더 |
| D2 | Phase 0 범위 | 권장안 전부 — 단 §2.3 보안 조정으로 S1·S2·S3·S4·CSP는 빠짐 | P0-02–P0-08 |
| D3 | 실행 도구·방식 | Claude Code 또는 Codex, Native(한 세션이 전부 구현) | 계획 머리말, §7 |
| D4 | 테스트 실행 정책 | (a) 과제 단위로 해당 테스트만, 전체 검증은 PR(묶음) 끝에 | P0-01이 AGENTS·CONVENTIONS·verification.md를 고침, §4.3–§4.4 |
| D5 | PR 단위 | (a) 위험 경계별 묶음 — D30에서 계획 여러 개를 묶은 PR 12개로 조정 | §6 |
| D6 | 세션 권한 | 세션 판단으로 모두 진행: push·PR·머지·후보 실행·릴리스 공개 | §4.8, P3-01(태그 전 사용자 필수 점검만 기다림) |
| D7 | 계획 위치·추적 | (a) 저장소 `docs/superpowers/plans/`에 두고 ledger 이슈 하나 | P0-01 Task 1·5 |
| D8 | Windows 실기 | (a) PR별 체크리스트, 사용자가 확인. 실행하지 않은 항목은 PASS가 아님 | §4.6, §4.9, P3-01 점검표 |
| D9 | v0.7 마이그레이션 | (a) Phase 1에서 제거(이번 v0.9.0에 반영) | P1-02–P1-05, ADR 0013 |
| D10 | 업데이트 서명 | (a·a·a)였으나 §2.3으로 하지 않음 | ADR 0016, 백로그 |
| D11 | OneDrive·junction | (a) cloud placeholder 허용, symlink·junction 차단 유지 | P0-05 |
| D12 | Knowledge 자동 저장 | (a) 기본 켬. 저널 DPAPI 통일은 §2.3으로 하지 않음 | P0-03 |
| D13 | 확인 절차 | (a) 되돌릴 수 있는 동작은 바로 실행 + 되돌리기. 네이티브 확인 대화상자는 §2.3으로 하지 않음(되돌릴 수 없는 삭제는 화면 안 확인 한 번) | P1-18, P2-07·P2-08·P2-09 |
| D14 | 제품 연결 | (a) 같은 설치의 제품은 기본 자동 연결, 설정에서 끔 | P0-04, ADR 0009 |
| D15 | 로그 | (a) 로컬 파일, 코드·component·method·지연만, 14일, 외부 전송 없음 | P0-07, ADR 0012 |
| D16 | Webhook 바이너리 본문 | (a) 기록·fixture 모두(base64 + 인코딩 필드) | P0-06 |
| D17 | 언어·테마 | (a) 메시지 카탈로그 구조만(한국어). 영어 UI·라이트 테마는 나중 | P1-11–P1-14의 이슈 카탈로그 |
| D18 | 타입 IPC | (a) component별 command + 공통 입장 함수 + ts-rs 12, Knowledge 시범 | P1-11–P1-14, ADR 0014 |
| D19 | lint·포맷 | (a) Biome 전체 포맷 + `.git-blame-ignore-revs` | P1-07(묶음 B4 단독), P1-08 Task 0 |
| D20 | 상태 관리 | (a) 라이브러리 없이 공통 hook | P1-15 |
| D21 | API Studio 저장소 | (a) native SQLite + localStorage 자동 이전, 파일 컬렉션은 Phase 2 | P1-17, P2-10 PR B |
| D22 | v0.7 시절 crate 이름 | (a) 의존성 alias만 도메인 이름으로 | P1-07 |
| D23 | 신규 의존성 | 승인. 실제로 쓰는 것은 `ts-rs` 12.0.1, `@biomejs/biome` 2(개발용). `tracing` 계열·`minisign-verify`는 쓰지 않음 | P1-11, P1-07 |
| D24 | devbox-agent | (a) Phase 1은 설계·RPC 준비, Phase 2에서 구현. 터미널은 Workspace에 남김(tmux·zellij로 충분) | P1-19, P2-01–P2-04, ADR 0015 |
| D25 | 신규 기능 순서 | Agent Hub → MCP 서버 → Git 보강 → API Studio 보강 | P2-05, P2-06, P2-07–P2-09, P2-10–P2-12 |
| D26 | 문서·프로세스 | (a) 오래된 기록은 보관 저장소로, 결정은 ADR로, workthrough 작성 의무 폐지 | P1-01, P0-01 |
| D27 | 작업 PC | (a) 집 PC(Ubuntu 26.04)만 | §3 |
| D28 | 툴체인 | (a) Rust·Node 모두 고정 | P0-01(Rust 1.98.1, Node 24) |
| D29 | 버전·릴리스 | 모든 작업이 끝난 뒤 버전을 한 번만 올린다(0.8.1 → 0.9.0). 중간 릴리스·태그·버전 변경 없음 | P3-01, §3, §5 |
| D30 | PR 묶음과 CI | 계획 43개를 PR 12개로 묶는다. 머지는 필수 CI(5–20분)만 기다리고 Windows 전체 acceptance(PF, 60–90분)는 머지 뒤 확인한다. 근거: 최근 PF 60회 중 성공 13·실패 16·취소 16, 같은 commit이 실패 뒤 재실행에서 통과한 경우 2건 → PR마다 PF를 기다리면 PR 43개에 대기만 100시간이 넘는다 | §4.0, §4.7, §4.9, §5, §6 |

2026-09-25 D12 추가 합의(ledger #580): P0-03의 저널은 `(vaultRoot, path)`로 구분하고 현재 노트 폴더 항목만 복구한다. 전체 8개 한도에서 다른 폴더 복구본을 자동 삭제하지 않는다. 명시적 버리기에는 화면 안 확인을 두고, 검증된 native 루트 캐시·활성화 시점 조회·비동기 순서와 복구 대상 확인을 적용한다. 저장용 폴더 파일 ID·새 schema 버전·변환 코드는 추가하지 않는다. 상세 계약은 P0-03 Task 4·5·7, 후속 타입 IPC는 P1-12에 있다.

### 2.3 보안 범위 조정(ADR 0016)

사용자 결정: "개인적으로 쓸 것이라 보안은 과하게 하지 않아도 된다. 이슈가 없을 정도면 된다."

- **하지 않는다:** 업데이트 manifest 서명(S1, D10), Authenticode 코드 서명(D10-3), 네이티브 확인 대화상자(S2, D13의 일부), multipart 파일 grant(S3), Suite pipe DACL 강화(S4), CSP 추가, 복구 저널 DPAPI 암호화(D12의 일부). 필요해지면 새 ADR로 다시 연다.
- **유지한다(실제 문제가 되는 것):** 활동 수집 개인정보 규칙의 fail-closed(B1, P0-02), 링크·junction 차단(P0-05), 비밀은 DPAPI로만 저장·평문 내보내기 없음(ADR 0008, P1-09), 로그에 내용·경로·비밀 없음(P0-07), 기록·파일 저장 전 비밀 정화(API Studio 기존 규칙, P2-10–P2-12), agent pipe peer 확인(P2-01), MCP 기본 꺼짐·쓰기 도구 별도 허용(P2-06), OAuth 토큰 봉인(P2-12), 인증서 검증 끄기의 상시 표시(P2-12).

### 2.4 백로그(이번 계획에 없음)

| 항목 | 출처 | 메모 |
|---|---|---|
| 업데이트 서명·Authenticode·pipe DACL·CSP·multipart grant·저널 암호화·네이티브 확인 | S1–S4, D10, D12, D13 | ADR 0016으로 보류 |
| PF 불안정 단계 안정화(hosted WSL2 VM 단계, 네 제품 명령 검증 단계) | D30, §5 | 재실행 한 번 규칙으로 버티고, 같은 단계가 계속 흔들리면 원인 수정 PR |
| WSL Control(`.wslconfig` 편집, 메모리·swap 감시, vhdx 압축, 배포판 백업) | review §8 후보 3 | Control Center 기능 후보 |
| Secrets & Environments(비밀 저장소 통합, `.env` 가져오기, 사용처 목록) | review §8 후보 4 | P1-09로 봉인기는 하나가 됨 |
| Knowledge 보강(전역 빠른 기록, 백링크 그래프, 프로젝트별 노트 연결, ADR 템플릿) | review §8 후보 7 | 자동 저장은 P0-03에서 완료 |
| API Studio: NTLM/Negotiate, 웹훅 터널(cloudflared), 웹훅 LAN 공유 비밀, 러너 반복·데이터 파일, 파일 컬렉션 양방향 동기화 | review §8 후보 5 | P2-10–P2-12 범위 밖 |
| 터미널을 agent로 옮기기 | ADR 0015 | 필요해지면 ADR 개정 |
| Launcher(명령 팔레트)를 agent 소유로 | review §8 | P2-04 트레이 이후 검토 |
| 영어 UI·라이트·고대비 테마 | D17-(b) | 카탈로그 구조는 P1-11–P1-14에서 준비됨 |
| `crates/integration` 계층 정리 | review §6 A4 | P1-05 이후 남은 소비자 기준으로 판단 |

P3-01 Task 5가 이 표에 각 항목의 현재 상태를 적는다.

## 3. 전역 제약(모든 계획에 적용)

- **언어:** 화면 문구·문서·PR 본문·ledger 댓글은 한국어. 코드 식별자·커밋 메시지는 영어. 커밋은 Conventional Commits `type(scope): subject`(scope 예: `devbox-workspace`, `devbox-api-studio`, `devbox-knowledge`, `devbox-control-center`, `suite`, `crates`, `workspace`, `frontend`, `release`). 1커밋 = 1과제.
- **환경:** Windows 11 전용(x64). 작업은 집 PC WSL(Ubuntu 26.04)의 `/home/jihoon/projects/devbox`. cargo 명령 전 `source ~/.cargo/env`. Rust 1.98.1, Node 24, pnpm 9(corepack) — P0-01 이후 `rust-toolchain.toml`·`.nvmrc`가 정본.
- **테스트 정책(D4):** 과제마다 실패하는 테스트를 먼저 쓰고 그 테스트와 직접 영향받는 테스트만 실행한다(`cargo test -p <crate> --lib <filter>`, `pnpm --filter <package> exec vitest run <file>`). 전체 검증(`pnpm verify:affected` — build·typecheck·test·clippy·fmt 포함)은 묶음 끝에 한 번(§4.4). `pnpm verify:all`은 릴리스 PR과 검증기 변경에만. `#[cfg(windows)]` 코드는 WSL에서 테스트되지 않으므로 CI `Rust (Windows)` 결과로 확인한다. 로컬 검증은 저장소의 자원 제한·worktree 간 잠금(CONVENTIONS §5)을 따른다.
- **의존성:** D23에서 승인한 것(`ts-rs` 12.0.1, `@biomejs/biome` 2) 말고는 새 의존성을 더하지 않는다. 계획이 "새 의존성 없음"이라고 한 곳에서 필요해 보이면 기존 crate·직접 구현으로 풀고, 그래도 안 되면 §4.10에 따라 멈춘다.
- **데이터 호환:** 저장 형식은 더하기만 한다(새 필드는 serde 기본값·TS 선택 필드). 기준은 공개된 v0.8.1이 쓴 데이터다(중간 릴리스가 없으므로 v0.8.x 데이터를 그대로 읽어야 한다). 사용자 데이터를 지우는 변경은 계획에 적힌 경우만(예: v0.7 가져오기 제거). 옛 형식 문서를 읽는 테스트를 같이 둔다.
- **버전·릴리스(D29):** 버전은 P3-01에서 한 번만 올린다. 다른 PR은 버전 번호·`CHANGELOG.md`·`docs/release-evidence.md`를 바꾸지 않는다. 후보 workflow(`windows-package-candidate.yml`)는 제품 버전과 태그가 같아야 하므로 P3-01에서만 쓴다. 릴리스 규칙: main CI가 끝난 뒤에만 태그, 후보와 태그 사이 머지 금지, 후보 뒤 7일 안에 태그, 실패한 후보·릴리스를 새 빌드로 덮지 않음, 공개 RC 없음, 공개 자산 7개 계약 유지. 릴리스 PR은 `.agents/skills/devbox-release/SKILL.md`와 `docs/release-policy.md`를 함께 따른다.
- **보안:** §2.3 범위. 비밀·토큰·경로·본문을 로그·IPC 투영·오류 메시지에 넣지 않는다.
- **UX:** 새 화면은 axe 위반 0(`@devbox/a11y/testing`). 되돌릴 수 있는 동작은 확인 없이 실행 + 8초 되돌리기(P1-18 `useUndo`), 되돌릴 수 없는 삭제·버리기는 화면 안 확인 한 번. 오류는 코드 → 문구 카탈로그(P1-11 이후 `Record<Issue, string>`).
- **Git:** main은 linear, squash 머지만. 브랜치는 §6 묶음 브랜치. main에 force push 금지. 커밋하지 않은 작업이 있는 파일을 `git checkout -- <file>`로 되돌리지 않는다(실험을 되돌릴 때는 먼저 커밋하거나 `git stash`).
- **기록(D26):** 작업 기록은 PR 본문과 ledger 댓글이다. `workthrough/` 파일을 새로 만들지 않는다. 사용자 실기 확인이 필요한 항목은 "사용자 확인 대기"로 적고 PASS로 적지 않는다(D8).
- **진행 방식:** 한 세션이 묶음 하나씩 진행한다. 필수 CI를 기다리는 동안 다음 묶음의 계획 파일을 읽어 둔다. PF(§4.7)는 기다리지 않으므로 머지 뒤 곧바로 다음 묶음을 시작한다. 서브에이전트는 쓰지 않는다(D3 Native). 단, Claude Code는 묶음 끝의 자체 리뷰(§4.4)를 위해 리뷰 서브에이전트 하나를 쓸 수 있다.

## 4. 묶음 실행 규칙과 PR 공통 절차

### 4.0 묶음 실행 규칙

- 묶음은 §6 첫 표의 한 행이다. 브랜치·worktree·PR은 묶음 단위다(§4.2). 계획 파일의 "Branch · PR" 절은 속한 묶음을 알려 준다.
- 묶음 안의 계획은 §6 두 번째 표의 순서대로 한다. 계획 안의 과제·Step·커밋 규칙은 그대로 따른다.
- 계획 파일 마지막 과제의 "§4.4–§4.9"는 **묶음의 마지막 계획에서 한 번만** 한다. 그 전 계획에서는 그 과제의 문서 갱신·확인 항목만 하고, PR 본문 초안(`/tmp/<묶음>-body.md`)에 그 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)을 더한다.
- 계획 파일이 "이 PR"이라고 쓴 곳은 묶음 PR을 뜻한다. "PR A"·"PR B"는 같은 묶음 안의 앞뒤 부분이다.
- 계획 하나를 끝낼 때마다 그 계획이 바꾼 패키지의 좁은 테스트가 모두 통과해야 다음 계획으로 넘어간다. 전체 검증은 묶음 끝에 한 번(§4.4).

### 4.1 시작 전 확인

```bash
cd /home/jihoon/projects/devbox
git status --short                     # main 체크아웃은 깨끗해야 한다(B1 머지 전에는 계획 폴더만 untracked)
git fetch origin main
gh run list --branch main --workflow CI --limit 1 --json conclusion,status,headSha   # 최신 main CI가 success
gh pr list --state open --json number,title,headRefName                           # 같은 파일을 건드리는 열린 PR이 없는지
git worktree list
```

- §6에서 이 묶음의 선행 묶음이 머지됐고, 앞 묶음의 PF 실패가 남아 있지 않은지(§4.9) ledger로 확인한다.
- 묶음에 든 계획 파일을 모두 처음부터 끝까지 읽는다(Global Constraints·Review Focus 포함). 계획이 가리키는 앞 계획의 Interfaces도 읽는다.

### 4.2 worktree 만들기

```bash
BRANCH=<§6의 묶음 브랜치>                  # 예: fix/suite/bootstrap-privacy-autosave-connection
NAME=devbox-${BRANCH##*/}                   # 예: devbox-bootstrap-privacy-autosave-connection (릴리스는 devbox-release-v0.9.0)
git worktree add /home/jihoon/projects/.worktrees/$NAME -b $BRANCH origin/main
cd /home/jihoon/projects/.worktrees/$NAME
pnpm install --frozen-lockfile
source ~/.cargo/env
```

- B1은 예외로 P0-01 Task 1이 이 worktree를 만들고 계획 폴더를 main 체크아웃에서 옮겨 온다.

### 4.3 과제 진행

- 과제 순서대로, 과제 안에서는 Step 순서대로(실패하는 테스트 → 실패 확인 → 구현 → 통과 확인 → 커밋).
- "실패 확인"에서 예상과 다른 이유로 실패하면(컴파일 오류 위치가 다름 등) 테스트를 고쳐 **예상한 이유로** 실패하게 만든 뒤 구현한다.
- **계획과 실제 코드가 다를 때:** 이름·경로·시그니처가 앞 계획으로 바뀌었거나 계획의 가정이 조금 틀렸으면, 계획의 의도(Goal·Interfaces·Review Focus)를 지키는 가장 작은 변경으로 맞추고 PR 본문 "계획과 다르게 한 점"에 적는다. 설계를 새로 해야 할 만큼 다르면 §4.10.
- 계획에 없는 개선·정리를 끼워 넣지 않는다(발견한 것은 PR 본문 "후속 후보"에 적는다).
- 커밋은 과제마다. 과제의 테스트가 통과하지 않은 상태로 다음 과제로 넘어가지 않는다.

### 4.4 묶음 검증

```bash
pnpm verify:affected            # build·typecheck·test·clippy·fmt (릴리스 PR은 pnpm verify:all)
git diff origin/main...HEAD --stat
```

- 실패하면 원인을 모두 고친 뒤 실패·영향 범위만 다시 실행한다(수정 하나마다 전체 재실행하지 않음).
- 자체 리뷰: `git diff origin/main...HEAD`를 묶음 안 계획들의 Review Focus, Global Constraints, §3과 한 줄씩 대조한다. 확인한 결과를 PR 본문 "검증"에 적는다.
- 계획들의 마지막 과제에 적힌 문서 갱신을 모두 했는지 확인한다(버전·CHANGELOG는 P3-01만 고친다).

### 4.5 push와 PR 만들기

```bash
git push -u origin $BRANCH       # 네트워크 오류면 2·4·8·16초 간격으로 최대 4번 재시도
gh pr create --title "<§6의 묶음 PR 제목>" --body-file /tmp/<묶음>-body.md
```

- 초안(draft) PR을 만들지 않는다(CI가 돌지 않음).

### 4.6 PR 본문 형식

```markdown
## 요약
- <이 묶음이 무엇을 왜 바꿨는지 1–3줄>

## 계획별 변경
### P?-?? <계획의 절 제목>
- 변경: <영역별 주요 변경>
- 계획과 다르게 한 점: 없음 | <차이와 이유>
- Windows 실기 확인: <항목> — 사용자 확인 대기(P3-01 후보 점검표에서 확인) | 해당 없음
(묶음 안의 계획마다 반복)

## 검증
- 과제 테스트: <계획별 요약>
- pnpm verify:affected: <PASS | 실패와 수정 내용>
- 자체 리뷰: 계획별 Review Focus 확인 결과
- 필수 CI: <머지 전에 결과로 갱신>
- Product foundation acceptance: 머지 뒤 확인(§4.9)

## 후속 후보
- 없음 | <계획 밖에서 발견한 것>

Ledger: #<번호>
```

본문 끝에는 실행 도구의 기본 표기(attribution) 줄을 붙인다(Claude Code: `🤖 Generated with [Claude Code](https://claude.com/claude-code)`). 모델 이름은 적지 않는다.

### 4.7 CI 대기와 수정

```bash
gh pr checks --watch --required
```

- 머지 조건은 필수 체크 3개 `Frontend (pnpm)`, `Rust (Cargo workspace)`, `Rust (Windows)`다(보통 5–20분).
- **`Product foundation acceptance`(PF, Windows 전체 acceptance, 60–90분)는 머지를 막지 않는다.** PR에서 자동으로 시작되고 머지 뒤에도 끝까지 돈다. 결과는 §4.9에서 확인한다. 예외: 릴리스 PR(P3-01)은 PF까지 기다린다.
- 필수 체크가 실패하면 `gh run view <run-id> --log-failed`로 원인을 찾고 고쳐 push한다. 같은 원인으로 3번 실패하면 §4.10.
- Windows에서만 실패하는 테스트는 `#[cfg(windows)]` 경로를 먼저 의심한다(WSL에서 돌지 않은 코드).

### 4.8 머지와 main CI

```bash
gh pr merge --squash --delete-branch
SHA=$(git ls-remote origin refs/heads/main | cut -f1)
gh run list --branch main --workflow CI --limit 3 --json databaseId,headSha,status,conclusion
gh run watch <main run id> --exit-status
```

- 머지 후 main CI가 끝날 때까지 기다린다. 실패하면 다음 묶음보다 먼저 고친다(`fix/<scope>/<topic>` PR, §4 절차, ledger에 원인).

### 4.9 기록·PF 확인·정리

- ledger 댓글: `Bn <PR 제목> — PR #<n>, merge <SHA 앞 12자>, main CI <run 링크>, PF <run 링크: 진행 중>, 포함 계획 <P?-?? 목록>, Windows 실기: <항목별 사용자 확인 대기/PASS/FAIL>`.
- 정리(CONVENTIONS의 순서): worktree가 clean이고 머지됐는지 확인 → `git worktree remove /home/jihoon/projects/.worktrees/$NAME` → `git worktree prune` → `git branch -D $BRANCH`(원격 브랜치는 `--delete-branch`로 이미 삭제) → `git -C /home/jihoon/projects/devbox pull --ff-only origin main`.
- **PF 확인(다음 묶음을 진행하면서):** 머지한 묶음 PR의 PF run(`gh run list --workflow "Product foundation acceptance" --branch <묶음 브랜치> --limit 1`)이 끝나면 ledger 댓글의 PF 칸을 갱신한다.
  - 성공 → 끝.
  - 실패 → 실패 단계가 §5의 불안정 목록에 있으면 `gh run rerun <run id> --failed`로 한 번 다시 돌린다. 다시 실패하거나 목록에 없으면 원인을 찾아 고친다: 진행 중인 다음 묶음의 첫 커밋으로 넣거나, 다음 묶음과 관계없는 곳이면 작은 `fix/<scope>/<topic>` PR로 먼저 머지한다.
  - 규칙: **앞 묶음의 PF 실패 원인을 밝히고 그 수정을 넣기 전에는 다음 묶음을 머지하지 않는다.** 수정이 다음 묶음에 들어 있으면 그 묶음의 PF가 확인한다.
- 중간 설치본이 없으므로 Windows 실기 항목은 "사용자 확인 대기"로 쌓이고, P3-01이 모아 후보 설치본으로 확인한다. 사용자가 그 전에 결과를 알려 오면 ledger를 갱신하고, FAIL이면 다음 작업보다 먼저 수정한다.

### 4.10 멈추고 묻는 경우

아래일 때만 멈추고, 상황·선택지·권장안을 짧게 정리해 사용자에게 묻는다. 그 밖에는 계획과 §3을 근거로 스스로 판단하고 PR 본문에 적는다.

1. 승인되지 않은 새 의존성이 꼭 필요하다.
2. 계획의 전제가 실제 코드와 달라 과제 여러 개를 새로 설계해야 한다.
3. 사용자 데이터를 잃을 수 있는 선택(형식 변경으로 옛 데이터를 못 읽게 됨 등)이 필요하다.
4. 같은 원인으로 필수 CI 또는 PF가 3번 실패했다.
5. 릴리스 후보·공개가 실패했고 원인이 코드 밖(권한·서비스 장애 등)이다.
6. P3-01 태그 전 사용자 필수 점검(응답을 받기 전에는 태그하지 않는다. "점검 생략"이라고 답하면 진행).

## 5. Windows acceptance 추적과 최종 게이트

- 묶음 PR마다 PF가 자동으로 돈다(제품·crate 경로를 건드리므로). 따로 단계 점검을 돌리지 않는다. 묶음 PF 결과는 §4.9 규칙으로 따라간다.
- 불안정 목록(같은 commit이 재실행에서 통과한 적이 있는 단계, 2026-09 기록):
  - `Workspace WSL2 and containers (isolated hosted VM)` › `Exercise the compiled artifact on an independent hosted WSL2 VM`
  - `Hidden products (Windows native)` › `Verify four-product commands and artifact workflows`
  - 새로 재실행으로 통과하는 단계를 보면 목록에 더한다(이 파일을 다음 묶음에서 고친다). 같은 단계가 묶음 두 개 이상에서 재실행이 필요했으면 원인 조사를 다음 묶음의 첫 과제로 넣는다.
- v0.7 legacy acceptance 단계는 B3(P1-02·P1-05)에서 지워지므로 그 뒤 PF가 조금 짧아진다.
- **최종 게이트(P3-01):** 모든 묶음의 PF 성공 기록 → main 최신 commit에서 `product-foundation.yml` 수동 실행 성공 → 릴리스 PR(PF까지 기다림) → 후보 → 태그 전 사용자 필수 점검 → 공개 → 공개 뒤 업데이터 점검.

## 6. 묶음·계획 색인

Ledger: #580

### 6.1 묶음 PR

| 묶음 | 계획 | 브랜치 | PR 제목 | 선행 |
|---|---|---|---|---|
| B1 | P0-01–P0-04 | `fix/suite/bootstrap-privacy-autosave-connection` | `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections` | — |
| B2 | P0-05–P0-08 | `fix/suite/files-webhooks-logs-describe` | `fix(suite): cloud files, webhook bodies, operation logs and one describe per session` | B1 |
| B3 | P1-01–P1-06 | `refactor/suite/archive-and-remove-v07` | `refactor(suite): archive history, remove v0.7 imports and unify dependencies` | B2 |
| B4 | P1-07 | `chore/workspace/mechanical-names-and-format` | `chore(workspace): rename engine crates and format the frontend with Biome` | B3 |
| B5 | P1-08–P1-10 | `refactor/crates/shared-platform-crates` | `refactor(crates): share process-tree, DPAPI, WSL helper and Markdown preview` | B4 |
| B6 | P1-11–P1-13 | `refactor/suite/typed-ipc-knowledge-api-control` | `refactor(suite): typed IPC for Knowledge, API Studio and Control Center` | B5 |
| B7 | P1-14 A·B | `refactor/devbox-workspace/typed-ipc` | `refactor(devbox-workspace): typed IPC for runtime, terminal, files, source and projects` | B6 |
| B8 | P1-15–P1-19 | `refactor/suite/hooks-streaming-store-undo` | `refactor(suite): shared hooks, terminal streaming, native API Studio store, undo and agent protocol` | B7 |
| B9 | P2-01–P2-04 | `feat/suite/devbox-agent` | `feat(suite): run runtime, webhooks and collectors in devbox-agent` | B8 |
| B10 | P2-05 A·B, P2-06 | `feat/suite/agent-hub-and-mcp` | `feat(suite): agent hub and Devbox MCP server` | B9 |
| B11 | P2-07–P2-09 | `feat/devbox-workspace/git-workflows` | `feat(devbox-workspace): branches, stash, hunks, amend, blame, conflicts and pull requests` | B10 |
| B12 | P2-10 A·B, P2-11, P2-12 A·B | `feat/devbox-api-studio/imports-runner-auth` | `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation` | B10 |
| R | P3-01 | `chore/release/v0.9.0` | `chore(release): prepare v0.9.0` | B1–B12 |

- B4는 기계적 변경(P1-07)만 담는다. 그 squash commit을 `.git-blame-ignore-revs`에 넣으므로(P1-08 Task 0) 다른 변경을 섞지 않는다.
- B11과 B12는 서로 파일이 겹치지 않아 순서를 바꿔도 된다(둘 다 B10 뒤).

### 6.2 계획 순서

| 순서 | 계획 | 계획 파일 | 묶음 |
|---|---|---|---|
| 1 | P0-01 | `p0-01-bootstrap.md` | B1 |
| 2 | P0-02 | `p0-02-activity-privacy.md` | B1 |
| 3 | P0-03 | `p0-03-knowledge-autosave.md` | B1 |
| 4 | P0-04 | `p0-04-suite-and-control-center.md` | B1 |
| 5 | P0-05 | `p0-05-filesystem-links.md` | B2 |
| 6 | P0-06 | `p0-06-webhook-transport.md` | B2 |
| 7 | P0-07 | `p0-07-diagnostic-logging.md` | B2 |
| 8 | P0-08 | `p0-08-describe-activation-cache.md` | B2 |
| 9 | P1-01 | `p1-01-history-archive-adr.md` | B3 |
| 10 | P1-02 | `p1-02-remove-v07-control-center.md` | B3 |
| 11 | P1-03 | `p1-03-remove-v07-knowledge-api-studio.md` | B3 |
| 12 | P1-04 | `p1-04-remove-v07-workspace.md` | B3 |
| 13 | P1-05 | `p1-05-shared-legacy.md` | B3 |
| 14 | P1-06 | `p1-06-workspace-dependencies.md` | B3 |
| 15 | P1-07 | `p1-07-mechanical-names-and-biome.md` | B4 |
| 16 | P1-08 | `p1-08-process-tree-crate.md` | B5 |
| 17 | P1-09 | `p1-09-secrets-dpapi.md` | B5 |
| 18 | P1-10 | `p1-10-wsl-helper-and-markdown-view.md` | B5 |
| 19 | P1-11 | `p1-11-typed-ipc-foundation.md` | B6 |
| 20 | P1-12 | `p1-12-typed-ipc-knowledge.md` | B6 |
| 21 | P1-13 | `p1-13-typed-ipc-api-studio-control-center.md` | B6 |
| 22 | P1-14 A·B | `p1-14-typed-ipc-workspace.md` | B7 |
| 23 | P1-15 | `p1-15-frontend-hooks-and-splitting.md` | B8 |
| 24 | P1-16 | `p1-16-terminal-channel-and-idle-loops.md` | B8 |
| 25 | P1-17 | `p1-17-api-studio-native-storage.md` | B8 |
| 26 | P1-18 | `p1-18-ux-undo-and-diagnostics.md` | B8 |
| 27 | P1-19 | `p1-19-agent-adr-and-protocol.md` | B8 |
| 28 | P2-01 | `p2-01-agent-skeleton.md` | B9 |
| 29 | P2-02 | `p2-02-runtime-in-agent.md` | B9 |
| 30 | P2-03 | `p2-03-webhook-in-agent.md` | B9 |
| 31 | P2-04 | `p2-04-collectors-tray-autostart.md` | B9 |
| 32 | P2-05 A·B | `p2-05-agent-hub.md` | B10 |
| 33 | P2-06 | `p2-06-mcp-server.md` | B10 |
| 34 | P2-07 | `p2-07-git-branches-stash.md` | B11 |
| 35 | P2-08 | `p2-08-git-hunks-amend-blame.md` | B11 |
| 36 | P2-09 | `p2-09-git-conflicts-gh-pr.md` | B11 |
| 37 | P2-10 A·B | `p2-10-api-studio-imports-file-collections.md` | B12 |
| 38 | P2-11 | `p2-11-api-studio-assertions-runner.md` | B12 |
| 39 | P2-12 A·B | `p2-12-api-studio-oauth-tls-codegen.md` | B12 |
| 40 | P3-01 | `p3-01-release-v0.9.0.md` | R |

## 7. 시작 프롬프트

다른 세션에 그대로 붙여 넣는다.

```text
/home/jihoon/projects/devbox 저장소에서 2026-09-23 리뷰 후속 작업을 진행해 줘.

1. docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md 를 끝까지 읽어.
   (첫 묶음 B1이 머지되기 전에는 이 폴더가 main 체크아웃에 untracked 상태로 있다. B1의 첫 계획 P0-01이 이 폴더를 저장소로 옮긴다.)
2. §6.1의 묶음을 순서대로 진행해. 묶음 하나 = PR 하나다.
   묶음 worktree → 묶음 안 계획들을 §6.2 순서대로(과제별 TDD와 커밋) → verify:affected와 자체 리뷰 → PR → 필수 CI → squash 머지 → main CI → ledger 기록 → 정리.
3. Product foundation acceptance(60–90분)는 기다리지 말고 머지한 뒤 결과를 확인해(§4.7, §4.9). 실패하면 다음 묶음을 머지하기 전에 원인을 고쳐.
4. 버전은 마지막 P3-01에서 한 번만 0.8.1 → 0.9.0으로 올린다. 그 전에는 버전·CHANGELOG를 건드리지 마.
5. Windows 실기 확인 항목은 "사용자 확인 대기"로 남기고 멈추지 마(P3-01에서 모아서 확인한다).
6. §4.10의 여섯 경우에만 멈추고 상황·선택지·권장안을 정리해 물어봐. P3-01은 태그 전에 사용자 필수 점검 응답을 기다린다.
7. Claude Code라면 각 계획 파일을 superpowers:executing-plans 스킬로 실행해(Native: 서브에이전트로 과제를 나누지 말 것).
   Codex라면 같은 순서를 직접 따라.
8. 진행 상태의 정본은 ledger 이슈와 PR이다. 계획 파일의 체크박스는 고치지 마.
```
