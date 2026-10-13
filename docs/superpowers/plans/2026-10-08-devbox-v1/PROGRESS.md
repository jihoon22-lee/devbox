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
| 단계 | S0 및 후속 #639 완료. S1 Task 1–9 구현·별도 검토 완료, S0b 범위 Windows 조기 스모크 및 Task 8 실제 Claude/Codex hook 확인 완료. S1 PR 최종 CI·자동 리뷰까지 계속 진행 |
| 다음 할 일 | S1 Task 10 출력 기반 대기 감지·자원·토큰 — 정확한 세션 연결과 닫힌 기록 고정, 관련 RPC 구현 |
| 동시 진행 | S2 뒤 한 세션이 두 트랙을 동시에(00-roadmap §3.6). 그때는 아래에 "트랙 1(S3→S4) 위치"·"트랙 2(S5) 위치" 두 줄을 쓴다 |
| 작업 중인 브랜치·worktree | `v1/s1-agents-terminal` · `../devbox-wt/v1-s1-agents-terminal` — `origin/main@fb6bc69d`에서 진행, Task 1–2 및 bridge/setup 수정 완료, 조기 스모크용 7eca14c5 push. 후속 실기 기록은 로컬 문서 커밋 |
| 마지막 갱신 | 2026-10-13 · Codex 실행 세션 |

## 2. 막힌 것 · 사용자 결정 대기

| 날짜 | 무엇 | 누구 결정 | 상태 |
|---|---|---|---|
| 2026-10-08 | P0 진행 방식 | 사용자 | 해결: PR·CI 없이 `docs/v1-plan`에 커밋·push(`aa5c6785`), main에는 S0b PR과 함께 |
| 2026-10-08 | PR 47개 → 하위 프로젝트당 1개(7개)로 줄임 | 사용자 | 해결(00-roadmap §3) |
| 2026-10-08 | S2 뒤 S3·S5를 동시에 진행할지 | 사용자 | 해결: 동시 진행. 한 세션이 하위 에이전트로 트랙 1(S3→S4)·트랙 2(S5)를 함께 돌림, 사용자가 세션을 더 띄우지 않음(00-roadmap §3.6) |
| 2026-10-09 | 사용자 확인 방식 | 사용자 | 변경(2026-10-13): 실사용은 계획 기능 완성 후 사용자가 진행하며 개발·머지·S1 착수 gate가 아니다. 필요한 개발 검증은 에이전트가 수행하며 사용자 테스트 요청 없이 확보된 자동 검증 근거로 S0 마무리(00-roadmap §2.2) |
| 2026-10-13 | 로컬 main 미푸시 커밋 `b2a508b6`·`1b84aa9d` | 사용자 정리 지시 | 완료: source-map-js 1.2.2는 S0b에 이미 반영됐고 필요한 ADR 링크 정리는 PR #638에 반영했다. 별도 머지하지 않고 두 로컬 커밋을 main에서 제거해 `origin/main@820576c9`와 일치시켰다(차이 0/0). 사용자 지시에 따라 추가했던 보존용 브랜치도 제거했으며 별도 파일 백업은 만들지 않았다 |
| 2026-10-08 | 저장소 `user.email`이 개인 Gmail이라 명령줄 push가 거절된다(GitHub 이메일 비공개). P0 커밋은 noreply 주소로 다시 써서 push했다. 저장소 설정을 `188744452+jihoon22-lee@users.noreply.github.com`으로 바꿀지 | 사용자 | 운영 해결: 커밋마다 `-c user.email=…`를 사용하며 저장소·전역 설정은 변경하지 않음(00 §3.3) |
| 2026-10-13 | 사용자 결정 2026-10-13 · 하위 에이전트 | 사용자 | 실행 하네스 제공 하위 에이전트를 사용, 과제마다 구현 → 별도 검토. 특정 모델 제한 제거 |
| 2026-10-13 | 사용자 결정 2026-10-13 · Windows 실경로 | 사용자 | 에이전트의 WSL→절대 경로 PowerShell CI 설치본 설치·실행·제거 승인. S1부터 각 S PR 전, 가능하면 S1 Task 2 뒤 S0b 조기 스모크. 출처/SHA256 확인, 소유 자원만 정리, 기존 v1/prod 발견 시 질문·UAC/재부팅/클릭 필요 시 중단 보고. v0.9/기존 데이터 보존, PASS·FAIL·미실행 구분(00 §2.2·01 §11) |
| 2026-10-13 | 사용자 결정 2026-10-13 · S2 전환점 | 사용자 | 기능 완성 뒤 실사용이 기본, S2 뒤 선택적 조기 사용 안내만 하고 기다리지 않음. 옮겼다고 알려 주면 날짜 기록, 문제는 이 표에 적고 진행 중 브랜치에서 수정 |
| 2026-10-13 | 사용자 결정 2026-10-13 · S3/S5 자원 | 사용자 | 동시 진행 유지. S2 Task 15b에서 전체 검사/E2E common-dir 잠금 --wait(최대60분 exit75) 구현, 기본 비차단 유지. 초과는 검사 대기 초과로 보고/재시도. 좁은 검사 잠금 없이 트랙당 jobs/threads/workers2·nice+10·지원 cgroup6/8GiB swap1GiB, S5 전용 target, 메모리 압박시 한 트랙 중단 |
| 2026-10-13 | 사용자 결정 2026-10-13 · R1/R2 | 사용자 | 30분 유휴 유지·WSL shutdown 재연결은 S6 공개 직전 사용자 확인으로 이동. R2 경과시간 기록. 그 전까지 미실행 |

## 3. 하위 프로젝트

| S | 계획 문서 | 상태 | 시작 | 완료 | 사용자(§2.2) |
|---|---|---|---|---|---|
| P0 | 00-roadmap §2 | 완료 | 2026-10-08 | 2026-10-08 | — |
| S0a | 02-s0a-spike | 완료(실기 미실행 기록 보존) | 2026-10-12 | 2026-10-12 | 결과 보고만 |
| S0b | 03-s0b-skeleton · 03b-s0b-app | 머지 | 2026-10-12 | 2026-10-13 | 실사용은 선택, 대기 없음 |
| S1 | 04-s1-agents-terminal · 04b-s1-app | 작업 중 | 2026-10-13 | | 완료·검증 결과 보고 |
| S2 | 05-s2-runs | 대기 | | | 완료·검증 결과 보고 |
| S3 | 06-s3-knowledge | 대기 | | | 완료·검증 결과 보고 |
| S4 | 07-s4-s6(→ 07a) | 대기 | | | 완료·검증 결과 보고 |
| S5 | 07-s4-s6(→ 07b) · S2 뒤 S3와 동시(트랙 2) | 대기 | | | 완료·검증 결과 보고 |
| S6 | 07-s4-s6(→ 07c) · S4·S5 뒤 | 대기 | | | **v1.0.0 공개 직전 확인(기다림)** |

## 4. PR (모두 7개)

하위 프로젝트 하나 = 브랜치 하나 = PR 하나다(00-roadmap §3). 머지하면 PR 번호와 머지 날짜를 적는다.

| PR | 브랜치 | 범위 | 상태 | PR 번호 | 머지 |
|---|---|---|---|---|---|
| S0b | `v1/s0b-skeleton` | S0b Task 1–24 (+ 계획 폴더) | 머지 | #638 | 2026-10-13 · `820576c9` |
| S1 | `v1/s1-agents-terminal` | S1 Task 1–24 | 대기 | | |
| S2 | `v1/s2-runs` | S2 Task 1–16 | 대기 | | |
| S3 | `v1/s3-knowledge` | S3 Task 1–12 | 대기 | | |
| S4 | `v1/s4-editor-git` | S4-1–S4-9 | 대기 | | |
| S5 | `v1/s5-tools` | S5-1–S5-9 | 대기 | | |
| S6 | `v1/s6-release` | S6-1–S6-6 → 태그 `v1.0.0` | 대기 | | |

P0와 S0a 결과는 PR 없이 `docs/v1-plan`에 커밋한다(00-roadmap §2 P0).

## 4.1 작업 묶음 (브랜치 안의 중간 지점)

묶음은 로컬 구현·좁은 검증·별도 검토의 진행 지점이며 과제별로 커밋한다. 전체 검증과 push·PR은 하위 프로젝트 끝에 한 번 수행한다(00-roadmap §3 운영 보정). 상태는 `대기`·`작업 중`·`완료(로컬)`·`push`·`막힘`이다. **작업 시간**은 구현·검토에 든 대략의 에이전트 작업 시간이며, S0b가 끝나면 일정 추정을 갱신한다.

| S | 묶음 | 과제 | 브랜치 | 상태 | 시작 | 끝 | 작업 시간 |
|---|---|---|---|---|---|---|---|
| S0a | — | Task 1–8 | `docs/v1-plan` | push(`58fda4d7`, 자동 부분) | 2026-10-12 | 2026-10-12 | 약 1.0h(구현·검토 합산 추정; 세션 경과 약 0.8h) |
| S0b | A | Task 1 | `v1/s0b-skeleton` | 완료(로컬) | 2026-10-12 | 2026-10-13 | 약 0.25h |
| S0b | B | Task 2–6 | `v1/s0b-skeleton` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약 0.5h(구현·검토 합산 추정) |
| S0b | C | Task 7–15 | `v1/s0b-skeleton` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약 1.5h(구현·검토 합산 추정) |
| S0b | D | Task 16–21 | `v1/s0b-skeleton` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약 1.0h(구현·검토 합산 추정) |
| S0b | E | Task 22–24 → S0b PR | `v1/s0b-skeleton` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약 1.5h(구현·검토·최종 통합 합산 추정) |
| S1 | F | Task 1–2 | `v1/s1-agents-terminal` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약 0.5h 경과(구현·별도 검토·영향 검사), 조기 CI/실기·발견 결함 수정 약0.5h 별도 |
| S1 | G | Task 3–4 | `v1/s1-agents-terminal` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약0.6h(구현·별도 검토·정책 수정·영향 검증) |
| S1 | H | Task 5–6 | `v1/s1-agents-terminal` | 완료(로컬) | 2026-10-13 | 2026-10-13 | 약0.9h(구현·별도 검토·실패 회귀 보완) |
| S1 | I | Task 7–10 | `v1/s1-agents-terminal` | 작업 중 | 2026-10-13 | | |
| S1 | J | Task 11–12 |  | 대기 | | | |
| S1 | K | Task 13–16 |  | 대기 | | | |
| S1 | L | Task 17–19 |  | 대기 | | | |
| S1 | M | Task 20–21 |  | 대기 | | | |
| S1 | N | Task 22–23 |  | 대기 | | | |
| S1 | 마무리 | Task 24 → S1 PR |  | 대기 | | | |
| S2 | 상세화 | Task 1 | `v1/s2-runs` | 대기 | | | |
| S2 | O | Task 2–6 |  | 대기 | | | |
| S2 | P | Task 7–10 |  | 대기 | | | |
| S2 | Q | Task 11–12 |  | 대기 | | | |
| S2 | R | Task 13–15b(동시 진행 준비 포함) |  | 대기 | | | |
| S2 | 마무리 | Task 16 → S2 PR |  | 대기 | | | |
| S3 | 상세화 | Task 1 | `v1/s3-knowledge` | 대기 | | | |
| S3 | S | Task 2–3 |  | 대기 | | | |
| S3 | T | Task 4–6 |  | 대기 | | | |
| S3 | U | Task 7 |  | 대기 | | | |
| S3 | V | Task 8–9 |  | 대기 | | | |
| S3 | W | Task 10–11 |  | 대기 | | | |
| S3 | 마무리 | Task 12 → S3 PR |  | 대기 | | | |
| S4 | 상세화 | S4-1 | `v1/s4-editor-git` | 대기 | | | |
| S4 | X | S4-2–S4-4 |  | 대기 | | | |
| S4 | Y | S4-5–S4-7 |  | 대기 | | | |
| S4 | Z | S4-8–S4-9 → S4 PR |  | 대기 | | | |
| S5 | 상세화 | S5-1 | `v1/s5-tools` | 대기 | | | |
| S5 | AA | S5-2–S5-3 |  | 대기 | | | |
| S5 | AB | S5-4 |  | 대기 | | | |
| S5 | AC | S5-5 |  | 대기 | | | |
| S5 | AD | S5-6 |  | 대기 | | | |
| S5 | AE | S5-7–S5-8 |  | 대기 | | | |
| S5 | 마무리 | S5-9 → S5 PR |  | 대기 | | | |
| S6 | 상세화 | S6-1 | `v1/s6-release` | 대기 | | | |
| S6 | AF | S6-1b–S6-2 |  | 대기 | | | |
| S6 | AG | S6-3 |  | 대기 | | | |
| S6 | AH | S6-4–S6-6 → S6 PR |  | 대기 | | | |

## 5. 계획과 달라진 점

실행 중 계획과 다르게 한 것을 적는다(컴파일 오류 수정, 이름 변경, 순서 변경 등). 다음 상세화 과제가 이 표를 읽는다.

| 날짜 | 위치(문서·과제) | 계획 | 실제 | 이유 |
|---|---|---|---|---|
| 2026-10-12 | 02 Task 1 | p50·p99 측정, 순차 쓰기/읽기 | p95와 원시 표본도 기록, Windows .NET 비동기 읽기·쓰기 및 FlushAsync, IO/종료 기한 적용 | 큰 echo의 양방향 버퍼 교착 위험을 줄이고 SC10 예비 판정 입력 확보. 초기 시간 초과는 PowerShell flush 누락으로 확인; 원래 Rust copy 경로도 작은 응답 회귀를 통과함 |
| 2026-10-12 | 02 Task 1 | 저장소 밖 실험 빌드 | Rust 1.98.1과 외부 전용 target, 공통 검증 잠금·jobs=2 사용 | 첫 red 검사 의존성 빌드는 상속된 공유 target과 잠금 없이 실행된 절차 이탈이 있었음. 이후 실행은 수정했고 공유 cache는 보존함 |
| 2026-10-12 | 02 Task 2 | journal의 activated=true로 socket activation 확인 | inactive→active 전환과 MainPID의 fd3 socket inode 대조 | 실험 probe 로그 필드를 바꾸고 재빌드하는 대신 실제 상속 descriptor로 확인. 대조 환경에서도 codex는 /usr/local/bin에서 발견되어 도구별 결과를 구분함 |
| 2026-10-12 | 02 Task 3 | 셸 PID와 unit 결과 측정 | systemd-run에 --expand-environment=no 사용, 소유 cgroup/PID만 대조 | 초기 harness의 $$가 systemd에서 $로 확장돼 실패. 실패 근거를 보존하고 명령 전달만 보정함 |
| 2026-10-12 | 01 §4.3 · 02 Task 4 | scope 프로세스는 tmux 종료에도 생존한다는 단정 | 별도 cgroup이어도 PTY 종료의 SIGHUP으로 끝날 수 있고, HUP 무시 프로세스는 남을 수 있음. 명시적 scope 종료 정책 유지 | 정상·HUP 처리 대역으로 실측. 데몬 재시작 중 tmux 세션 유지 기준은 통과했으며 구조·범위를 바꾸는 대체 경로가 아님 |
| 2026-10-12 | 02 Task 4 | 실제 PTY/scope 측정 | private tmux socket·소유 PID/cgroup·기한 있는 입출력으로 검증 | 초기 0.6초 CPU probe의 공통 잠금 누락을 기록. 후속 수명 검사는 잠금을 사용했고 통과한 지연 측정을 반복하지 않음 |
| 2026-10-12 | 01 §9.2 · 04 Task 8·10·resume | Claude Notification로 waiting, Codex ENV/cwd 또는 최신 파일로 완료·재개 연결 | PermissionRequest 즉시 waiting + 알림 fallback; Codex 정확한 source/주 thread/현재 turn gate와 bounded retry/진단, 필수 회귀 | 실제 TUI에서 6,018ms 지연 알림·동일 ENV 자동 제목 완료 관찰. 구조·목표 변경 없이 source 계약 보정; S1 구현 검증은 미완료 |
| 2026-10-12 | 02 Task 7 · 03b Task 24 확인 7 | 트레이에서 30분 유휴 | 설치 창 열고 연결 유지 30분; R1 확인 7/R2 확인 4·8 유보, 미실행 | 트레이는 S1 Task 23 범위. 조기 구현하지 않음. 당시 유보는 2026-10-13 결정으로 대체, 실기 미실행 기록만 보존 |
| 2026-10-13 | 03 Task 1 | 새 저장소 골격과 초기 CI | CONVENTIONS 유지·갱신, 실재 protocol member만 등록, 공유 cache sweep는 보고만 수행 | 현재 명시된 규약·자원 보호를 보존. 전체 검사·push·squash 정책은 00 §3 보정이 과거 예제보다 우선함 |
| 2026-10-13 | 03 Task 1 검증 | 초기 script/CI 검사 | 실제 workflow shell을 쓰는 Git rename·잘못된 기준·grep 오류 회귀 포함 4개 통과, actionlint·syntax·metadata 통과, CI에 회귀 등록 | 코드→문서 rename 누락과 scope 조회 오류의 묵시적 성공을 수정. 상수 값만 비교하는 의미 없는 stub 테스트는 제거함 |
| 2026-10-13 | 03 Task 2 | oversized stream 시험 길이 | 프레임 헤더 5 + stream ID/flags 5 + chunk 상한 + 1 | stream 본문 overhead 누락 보정. 부분/복수 프레임·상한·poisoning·encode 보존·채널 helper 10개 통과, 별도 검토 통과 |
| 2026-10-13 | 03 Task 3 | tagged enum derive와 WireError detail Value | RawValue 봉투 직접 역직렬화·detail RawValue·TS optional/unknown; 누락과 실제 null 구분 | serde tagged enum의 중간 버퍼가 RawValue 역직렬화를 지원하지 않음. 메시지·정밀도·null·frame·TS 계약 7개 통과, 별도 검토 통과. Task 4의 to_wire·동등성 예제도 이에 맞춰 보정 |
| 2026-10-13 | 03 Task 4 | Value 기반 오류 변환·DispatchError 동등성 derive | 직접 직렬화 후 WireError 역직렬화, 동등성 derive 제거 | Task 3 RawValue 계약에 맞춤. 라우터 7·선언/주제 1·오류 2개 통과, 실행 전/중 취소와 내부 원인 비노출·별도 검토 통과 |
| 2026-10-13 | 03 Task 5 | splitter의 5바이트 대기·채널/재시도 counter 증가 | 4바이트 길이 즉시 검사·malformed 뒤 poisoning, BytesMut 분할, active ID 회피·backoff 포화 증가 | 예제의 4바이트 zero 회귀와 일치시키고 채널 충돌·counter overflow 방지. mux 12개 통과, 별도 검토 통과 |
| 2026-10-13 | 03 Task 6 | 타입 생성기·묶음 끝 전체 검사/push | 생성기 7개 검사·실제 생성/--check·별도 검토 통과, CI에 check 연결; 묶음 B는 로컬 완료 | 중복 주제·잘못된 CLI 인자도 거부. 현재 운영 규칙에 따라 전체 검사·push는 S0b 끝에 수행 |
| 2026-10-13 | 03 Task 7 · 01 인스턴스 계약 | 이름 정규식·환경 변수 경로·uid=0 fallback | 공유 bin 경로와 충돌하는 `bin` 예약 거부, HOME 절대 경로 필수·빈/상대 XDG fallback, UID 실패 명시 진단 | 경로 10개 통과 뒤 검토에서 bin 충돌 발견. 이름 회귀 red→green 3개(8개 제외), 수정 재검토 통과. 소유 temp fixture만 사용 |
| 2026-10-13 | 03 Task 8 | 병합 뒤 typed 변환·단순 tilde join | 각 파일 먼저 strict 검증·section table 확인, 반복 slash를 HOME 하위로 확장 | local override가 shared 오류를 숨기지 않고 serde의 positional section 허용을 막음. 설정 범위 9개 통과 뒤 ~// HOME 이탈 발견·해당 회귀 1개 red→green, 수정 재검토 통과 |
| 2026-10-13 | 03 Task 9 | mpsc·AtomicBool·수신 시 큐 drain | bounded VecDeque·구독별 짧은 mutex·Notify, overflow/Resync/새 발행을 같은 잠금으로 순서화 | drain 중 발행 경쟁·오래된 이벤트·무한 drain 위험 제거. 허브 6개(64회 발행/수신 경쟁·등록된 대기 깨우기 포함) 통과, 별도 동시성 검토 통과; 단위 기한을 성능 근거로 쓰지 않음 |
| 2026-10-13 | 03 Task 10 · 01 DB 계약 | writer/read pool·마이그레이션 전 이전 백업 삭제 | 읽기 전용 연결·checked rev·panic/취소 경계, migration 순서 검사; VACUUM snapshot·batch/소유 receipt 동기화 후 원자적 게시·이전 소유본만 정리 | 초기 DB 8개 통과 후 semver/symlink·새 batch/receipt 충돌 회귀를 추가해 총 12개, 변경된 백업 범위만 재검사. 검토의 같은 앱 버전 stale backup·미소유 temp 삭제 2건 수정 후 재검토 통과 |
| 2026-10-13 | 03 Task 10 복구 경계 | 예제에 게시 중단 경계 없음 | snapshot 게시와 receipt 게시 사이 중단 시 복구본 보존. 첫 게시의 미소유 target은 재시도에서 안전 중단; 기존 소유본 교체의 재시도는 아직 migration 전인 DB에서 다시 snapshot | 파일·디렉터리 sync 순서를 구현. 실제 강제 종료/전원 중단 실기는 하지 않았고 Windows directory durability는 검증 대상이 아닌 Linux core 경계임 |
| 2026-10-13 | 03 Task 11 | 초기화 오류 무시·같은 시각 crash 덮어쓰기 | 초기화 직렬화·직접 global subscriber 설치·오류 반환, create_new 접미사·panic ring try_lock | 필터 3개를 소유 자식 프로세스에서 통합 검사 1개로 확인, collision/busy ring 단위 2개 통과. scoped subscriber 오판과 try_init의 부분 초기화 결함 수정 후 재검토 통과. 일 단위 최대 7개·최근 50개·guard flush 확인 |
| 2026-10-13 | 03 Task 12 | 프로젝트 CRUD/scan·거절을 Ok(false)로 반환·await 뒤 이벤트 발행 | typed write 거절 rollback, writer의 커밋 직후 callback으로 이벤트 발행; canonical UTF-8·별칭 dedup·blocking IO 분리 | 프로젝트 7개 및 writer 영향 필터 통과, TS 생성/check 통과. 취소된 waiter의 커밋 후 이벤트 누락 회귀 red→green·재검토 통과; 백업·무관한 생성기 검사는 반복하지 않음 |
| 2026-10-13 | 03 Task 13 | 소켓 삭제·단순 fd3 채택·사후 dedupe·구독 등록 | data-dir lease와 소켓 receipt, PID/type/family/path/CLOEXEC 검증·abstract notify; in-flight 결과 공유·채널 generation·구독 overflow Resync·해제 | 데몬 통합 19개, route/dedupe 2개·client cleanup 2개·channel cleanup 2개 통과. 소유 fixture 전달 소켓으로 검증했으며 실제 systemd unit 변경 없음. 별도 최종 검토 통과 |
| 2026-10-13 | 03 Task 13 취소·종료 계약 | 기다리는 future와 mutation 수명 혼합 | 수용한 mutation은 waiter 취소 후에도 실행·결과 보존, outer request와 실행 task를 함께 추적해 lease 해제 전에 drain | clientRequestId의 params 비교는 raw JSON 텍스트 기준. pending subscribe 취소는 ack 경쟁의 서버 구독 유출을 막기 위해 해당 연결 종료. 라이브 구독 Drop은 Unsubscribe 전송·로컬 제거 |
| 2026-10-13 | 03 Task 14 | Tokio stdio copy·WS 프레임 누적·단일 loop send | 독립 pipe OFD/AsyncFd로 양쪽 취소 가능, 5초 활성화 전체 기한·stdout 격리; WS 메시지별 단일 프레임·독립 양방향·토큰 URL 인코딩 | 브리지 7개·게이트웨이 6개·부모 pipe flag 보존 1개 통과. blocked stdout 종료 red→green, 8MiB raw/16MiB WS 양방향·강제 종료 후 flags 보존 확인. fake systemctl만 사용, 별도 재검토 통과 |
| 2026-10-13 | 03 Task 15 | 고정 경로 unit·단순 env/which·실기와 묶음 전체 검사 | private atomic env·단계 실패 중단·instance XDG drop-in·실제 권한 검사, user manager 응답/linger 진단; 실제 설치는 Task 24 | setup 7개·doctor 4개·route 1개·TS 생성/check 통과. 실제 systemd parser가 quoted ListenStream을 거부하는 결함과 owner 실행권한 오판 각각 red→green 후 재검토 통과. Linux 검증은 systemd-analyze 필요, 서비스는 변경하지 않음 |
| 2026-10-13 | 03·03b 운영 절차 | 묶음별 전체 검사/push·ci-ok 단일 보호 전환·rebase | 현재 사용자 규약대로 과제별 좁은 검사, Task 24 통합 검증/push, 기존 필수 CI 3개 유지·squash(당시 S0 확인 gate는 최신 §2 결정으로 대체) | 수용·머지 전 완료로 기록하지 않도록 순서도 정리. 구현자와 별도 검토 완료는 로컬 진행이며 PR/Windows 실기 통과와 구분 |
| 2026-10-13 | 03b Task 16 | UI 골격·기본 컴포넌트·고정 높이 없는 Dialog | React/Vite 설정·토큰·native 버튼·Radix 포커스·복사 실패 처리, 화면 높이 제한과 스크롤 | UI 13개 red→green, 별도 검토의 긴 대화상자 버튼 잘림 수정 후 재검토 통과. CSS 수정 뒤 무관한 통과 검사는 반복하지 않음. 전체 typecheck/build·실기는 Task 24 |
| 2026-10-13 | 03b Task 17 · 01 §5.5 R6 | 느슨한 프레임·구독 직후 resync·종료와 전송 실패 혼합 | Rust 상한·봉투 검증, 구독 ACK 후 ready/resync, 늦은 ACK 해제·협력 취소·재시도 수명 분리 | 프레임 5·클라이언트 30·WebSocket 7·오류 문구 1개 통과. 전송 실패 후 복구 red→green, 별도 검토 통과. fallback 재시도 timer와 오래된 socket callback 정리 확인 |
| 2026-10-13 | 03b Task 18 | queryFn 캐시 쓰기·clear만 수행·연결 수명 누락 | Query 커밋 시 rev 비교·조회 중 이벤트 취소/재조회, ACK 후 전체 무효화·데몬 변경 시 관찰자 재연결·StrictMode 수명 정리 | helper 1·실제 RPC/React 통합 8개 통과, 별도 검토 통과. 같은 데몬의 마지막 데이터 유지·최신 수동 캐시 취소 보존·이전 데몬 응답 차단·클라이언트 교체 확인 |
| 2026-10-13 | 03b Task 19 | 프레임·라우팅·키맵·팔레트·섹션 오류 | 저장 경로 허용목록·gateway 쿼리 유지, 키 capture/IME/모달 보호·팔레트 접근성·확정 route 기반 오류 초기화 | 새 테스트 16개 통과, 별도 최종 검토 통과. 실제 프레임 오류 뒤 이동 경쟁과 터미널 이벤트 차단 회귀 수정. 키 예외는 01 §8.5의 문자 제한을 따름. 프로젝트·Tauri 연결은 다음 과제 |
| 2026-10-13 | 03b Task 20 | 프로젝트 전환·추가·제거·선택 유지 | 공유 구독·키보드 grid·진행 중 창/선택 유지·진단 복사, 제거 rev 기반 재선택 차단 | 새 테스트 총 11개 통과, 별도 최종 검토 통과. 지연/실패 재조회와 창 재열기 회귀 수정, 기존 목록 rev 보존. interactive option 대신 [WAI-ARIA grid 권고](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/)에 맞춰 행별 동작을 구분 |
| 2026-10-13 | 03b Task 21 | 개발·검증·E2E·CI 진입점 | 공통 잠금/자원 제한·기존 데몬 보존·소유 stdout 준비 확인·격리 fixture, 필수 Frontend/Linux Rust 이름 유지 | Python 회귀 총 10개·대상 E2E/config TS·actionlint·구문 검사 통과, 별도 재검토 통과. 추가 완료 전 Escape 경합과 상대 runtime 경로 수정. 실제 E2E·전체 check는 Task 24에서 수행 |
| 2026-10-13 | Windows 사전 확인 | 로컬 native 빌드 도구 | Node만 확인, cargo/rustc/pnpm·기본 cargo·vswhere 미확인 | 별도 설치 없이 계획의 hosted Windows CI에서 native 검사와 설치 파일을 생성한다. 로컬 Windows 빌드 성공으로 보고하지 않음 |
| 2026-10-13 | 03b Task 22 | Tauri 껍데기·창 크기·self-check | Windows 의존성 분리·자체 lock·단일 인스턴스·COM 성공 뒤 창 표시, 물리 장식 포함 크기 제한 | 순수 geometry 3·리소스 2개 red→green, 스키마/metadata/태그 아이콘 확인·최종 별도 검토 통과. 작은 작업 영역의 장식 초과 수정. 실제 Windows 컴파일·UI·번들 위치는 Task 24까지 미검증 |
| 2026-10-13 | 03b Task 23 | WSL 배포·게이트·브리지·창별 IPC | 소유 digest 배포·인스턴스 setup/doctor·단일 취소 supervisor·bounded duplex·Job 정리, attachment/revision 수명·오류 복구 UI | 새 Rust 19·프론트 14·직접 영향 시작 검사 3개 통과, 별도 최종 검토 통과. 반복 종료의 정리 우회·진단 소실·최초 연결 뒤 복구 UI 누락을 수정. 모든 실행 fixture는 소유 임시 자원이며 실제 WSL 설치/native Windows는 아직 미실행 |
| 2026-10-13 | 03b Task 23 후속 | 사용자 지정 XDG 배포 경로 | publish·setup·doctor·bridge가 절대 XDG_DATA_HOME 또는 HOME 기본 경로를 함께 사용 | 새 회귀 4개(기존 구현에서 행동 실패 3개 확인) 포함 deploy 6개 통과·별도 검토 PASS. 소유 HOME/XDG fixture만 사용, symlink·기존 인스턴스 데이터 보존 확인. Windows 실기는 여전히 미실행 |
| 2026-10-13 | 03b Task 24 | Windows 스크립트·CI·dogfood | 소유 원본 결합 Windows 동기화, musl 리소스 출처·native 검사·NSIS·self-check, 정확한 원격 SHA/run/설치 파일 해시 확인, E2E 성공 화면 artifact·문서 갱신 | PS5.1 소유 fixture 3종·Python 관련 5개·actionlint/문법/수정 TS Biome 통과. 최초 PR workflow 조회 결함을 재현 후 저장소 실행 API 조회로 수정, dogfood 2개 재통과. 별도 최종 검토 PASS. 최종 검사 실패를 수정 후 영향 범위 재검사: Rust workspace 146·앱 core 28·Python 12·프론트 106·실제 E2E 3개 통과(수정된 프론트 테스트 18개 재통과), fmt/clippy/생성 타입/Biome/typecheck/build 통과. E2E 화면 3장 시각 확인. native Windows는 CI에서 실행 예정 |
| 2026-10-13 | S0b 최종 통합 검사 | 실패 범위 수정·재검증 | stale doctor 기대값, Node 24 ESM JSON import, 엄격한 테스트 타입, import 정렬·Clippy 규약 보완. 테스트 assertion 의미와 정상 동작 유지 검토 | 전체 check 1회 뒤 실패/영향 범위만 재실행. 초기 53.1초·메모리 peak 1.19GB, Rust/E2E 재검증 29.5초·peak 1.89GB. 자원 제한 준수. 실제 WSL 하네스의 임시 runtime 설정 오류는 실패·소유 파일 보존으로 기록 후 exact-content 검증 정리하고 실제 manager 경로로 보정했다 |
| 2026-10-13 | S0b WSL 실제 수용 | 고유 인스턴스 setup·doctor·systemd·bridge·프로젝트 CRUD·EOF | `test-s0b-1c4aa0e8ee81`, 실제 사용자 manager + 소유 임시 XDG 데이터, 현재 debug CLI SHA256 `f2332c8759ec398c11d6bdd01e29eea878448991be8b30548cc801658532fe6b` | PASS. doctor systemd/tmux/env/도구 정상, 기존 linger·inotify는 warn 그대로 유지. 전용 unit·enable link·소켓·임시 데이터 정리와 잔존 unit 없음 확인. 근거: Git common-dir `devbox-verification/s0b-wsl-acceptance.json`; Windows 설치본/R1/R2 미실행을 대신하지 않음 |
| 2026-10-13 | PR #638 첫 CI 보정 | CI 색상 로그·Windows Clippy | `fb93b565`의 CI `38289821740`: Frontend·Rust 성공, E2E는 ANSI 포트 강조 때문에 준비 판독 실패. Windows `38289822012`: Linux 리소스 성공, native Clippy 7건으로 후속 테스트/NSIS/self-check 중단 | 누적 로그 ANSI 제거·정확한 Local 주소 검사로 수정, 회귀 Vitest 8·Python 4와 `FORCE_COLOR=1 CI=true` 실제 E2E 3·tsc/Biome 통과. Windows let-chain·test 배치·동일 UTF-16 chunk 처리 보완, 영향 core 6개와 순수 앱 Clippy 통과. 재검토 후 최종 변경 CI 예정; 이전 실패를 성공으로 취급하지 않음 |
| 2026-10-13 | PR #638 버전 불일치 리뷰 | Welcome 전달 전 연결 종료 | 실제 서버 버전 Welcome을 writer flush ACK 뒤 종료하도록 보완(전체 대기 2초 제한), 중복 Hello·다른 인스턴스는 계속 우선 거부 | 실제 Unix 소켓 RED 재현 → channel 4·server lifecycle 2 검사 통과, 별도 검토 PASS. 불일치 연결은 권한을 얻지 않으며 pipelined mutation도 실행되지 않음. 후속 클라이언트 2건과 함께 최종 영향 검증 예정 |
| 2026-10-13 | PR #638 클라이언트 리뷰 | Welcome 제한 시간·동일 의도 재시도·오프라인 RPC | Hello 뒤 5초 제한과 연결 세대 방어, 동일 데몬의 응답 불명 요청만 같은 ID로 재시도. 입력 변경·취소·reset·unmount·확정 응답은 의도 종료. 호출 시 canonical params snapshot으로 중복 비교 일치. 로컬 RPC는 브라우저 offline에도 즉시 실행/실패 | RED 회귀 후 client 37·query/UI 37 통과, 별도 검토 PASS. 최종 영향 검사: daemon 21·Clippy/fmt·Biome·E2E 3 통과. 테스트의 엄격한 배열 접근 타입 5곳 보정 후 query 24·tsc/build/Biome 통과. 새 최종 SHA의 CI·Windows 설치 파일은 PR 본문에 기록; 당시 미머지. 최신 운영은 §2의 2026-10-13 결정 참조 |
| 2026-10-13 | S0b 최종 CI·마무리 결정 | `c3212af815dca25bd11d290f32110d8157e9ebe6` | [CI 38294731916](https://github.com/jihoon22-lee/devbox/actions/runs/38294731916)·[Windows 38294732514](https://github.com/jihoon22-lee/devbox/actions/runs/38294732514) 전체 성공, 설치 파일 출처 확인 | 구현·자동 검증 완료. 실기 미실행 사실은 보존하며 사용자 테스트 요청·대기 gate로 두지 않음. 필요한 개발 검증은 에이전트가 해당 단계에서 수행. 당시 보존용 브랜치를 만들었으나 후속 사용자 지시로 제거했다(§2). 필요한 ADR 정리 반영. 이 행은 머지 전 기록이며 최종 결과는 아래 행 참조 |
| 2026-10-13 | S0 마무리·S1 준비 | 최종 PR `f368df36` → squash `820576c9` | [CI 38312265408](https://github.com/jihoon22-lee/devbox/actions/runs/38312265408)·[Windows 38312265384](https://github.com/jihoon22-lee/devbox/actions/runs/38312265384) 모두 성공, PR/main tree OID 일치 확인. main 전환 뒤 남은 폐기 제품의 생성 schema 20개(1.84MB)를 식별·제거해 0 ahead/0 behind·clean 확인. S0 전용 worktree와 로컬·원격 브랜치 제거 | 미푸시 두 커밋은 필요한 변경을 반영한 뒤 main에서 제거, 보존용 브랜치·별도 파일 백업 없음. S1 전용 worktree를 최신 main에서 만들고 frozen 의존성 준비. Task 1의 기존 Hello flush ACK·채널 세대 취소·중복 요청 경계를 유지하며 credit 소진/재개·채널 공정성·종료 회귀부터 구현 예정 |
| 2026-10-13 | PR #638 머지 후 자동 리뷰 | [DB 최신 스키마 거부](https://github.com/jihoon22-lee/devbox/pull/638#discussion_r4252644638) | 리뷰는 머지 뒤 도착. 구버전이 더 높은 도메인 버전을 건너뛰고 DB를 여는 결함으로 확인. 전체 도메인을 먼저 검증해 알려진 최고 버전 초과·알 수 없는 도메인을 거부하며 journal mode·기본 metadata·다른 migration·backup보다 먼저 실패 | RED 2개 재현 후 독립 검토에서 단독 WAL reader 종료 시 checkpoint 결함도 RED 재현·수정. 버전 검증 중 close-checkpoint 차단, 지원 확인 후 기존 동작 복원. DB 18·projects 7·daemon 21 검사 및 Clippy·fmt·Biome 통과, 독립 재검토 PASS. DB/WAL byte 보존·다른 도메인 미변경·정상 교차 도메인 upgrade 확인. 후속 [PR #639](https://github.com/jihoon22-lee/devbox/pull/639): [CI 38315340950](https://github.com/jihoon22-lee/devbox/actions/runs/38315340950)·[Windows 38315341119](https://github.com/jihoon22-lee/devbox/actions/runs/38315341119) 성공 및 [자동 코드·보안 리뷰](https://github.com/jihoon22-lee/devbox/pull/639#issuecomment-6133569350) 최종 SHA 일치·완료·추가 지적 없음 확인 뒤 `fb6bc69d`로 squash 머지. 원래 리뷰 해결, 수정용 worktree와 로컬/원격 브랜치 정리 완료 |
| 2026-10-13 | A·B 사실 확인 | 낡은 운영 예제·S1 계약·헬퍼 | 00/04/04b/05/06/07의 묶음 push·rebase·사용자 전달·sed 패턴 잔존 확인. 보호 규칙 API는 Frontend (pnpm)·Rust (Cargo workspace)·Rust (Windows) 3개 유지. 서버 Outgoing frame/generation/flushed·dedupe·Hello flush ACK·세대 취소 존재, 지정 6개 헬퍼는 미구현 | 문서 정합화 대상. 헬퍼는 Task 8에서 생성하며 이번에는 구현하지 않음. S0b 브랜치 전체 검토는 기록 없어 수행 주장하지 않음 |
| 2026-10-13 | A3 일정 재추정 | S0b 20–28h | PR #638 commits API: 78c2d3d9 15:00:35Z → c3212af8 19:00:15Z 약4h, 이후 CI/리뷰·마무리 약3h. 구현창에는 18:26 이후 리뷰 수정도 섞여 있어 순수 구현 시간 계측과 다름. 남은 S1 14–22h, S2 10–16h, S3 8–13h, S4 8–13h, S5 10–16h, S6 5–9h | S1 tmux/PTY/hook/Windows 실통합 위험으로 비율 일괄 축소 안 함. 순차55–89h, S2끝24–38h. S3/S4/S5 순차26–42h→병렬16–26h, 추가 잠금/통합2–4h 고려 순절감6–14h, 남은 최장경로47–77h. 상세화/연결 구현은 단계 예산 포함, 사용자 승인 대기 제외 |
| 2026-10-13 | A6 GHSA-wrw7-89jp-8q8g | glib Windows 영향 재확인 | app/src-tauri에서 cargo tree --locked -i glib --target x86_64-pc-windows-msvc exit0, nothing to print. 경보 #14 not_used dismiss 완료, root lock #1은 fixed | ADR0016 개인용 범위와 지원 Windows graph 미포함. GTK/WebKit용 glib 0.18.5만 남음. Linux GUI 지원 시 재검토 |
| 2026-10-13 | A6 Dependabot 실패 | SHA820576c9 업데이트 job | [run38312818489/job115022349175](https://github.com/jihoon22-lee/devbox/actions/runs/38312818489/job/115022349175): security_update_not_possible, latest-resolvable0.18.5/lowest-non-vulnerable0.20.0. GTK0.18/WebKit2GTK2가 glib0.18 요구 | CI 고장 아님. Windows graph 미포함이므로 S1에 GTK 스택 업그레이드 추가 안 함 |
| 2026-10-13 | A7 계획 브랜치 정리 | clean·main 통합 확인 후 제거 | docs/v1-plan@58fda4d7 clean, PR638 head f368df36의 조상, head tree와 squash820576c9 tree 모두 d22d0e0b. worktree 제거→prune→로컬·원격 docs/v1-plan 삭제 완료 | squash라 main 조상 검사가 아닌 PR 조상+최종 tree 동일성으로 통합 확인. S1 05c13913 보존. 사용자 자료/공유cache 변경 없음 |
| 2026-10-13 | A·C 문서 정합화 검증 | S1 선행 문서만 변경 | 11개 문서에 운영·실기·일정·후속 과제 반영. 독립 검토 지적(최종 SHA 예외/S2 강제전환 잔존)을 수정하고 재검토 통과. Biome CI 83 files 통과 | 새 보고서/제품 코드 없음. Windows 스모크는 아직 미실행. 실제 Windows 영향 없는 glib 업데이트와 Task8 헬퍼 선행 구현은 하지 않음. 기존05c13913 보존·문서 로컬 커밋 후 Task1 시작 |
| 2026-10-13 | 04 Task 1 스트림·Outbox | sync StreamSink·단순 프레임 큐·전체 chunk 신용 대기 | Outgoing frame/generation/flushed·Hello flush ACK·dedupe·채널 취소 보존, bounded256/stream240(JSON16 여유) async enqueue, 가용 신용 부분 예약·256KiB 포화·64KiB 분할, async finish와 FIN 순서. debug.echo는 protocol 계약/TS 생성, test-*에서만 라우팅 | API 부재 RED 뒤 구현. 작은조각300개 Outbox/클라이언트 회귀 RED→수정, 입력 overflow는 해당 스트림 FIN. 최초 직접 영향43개·gen-ts/check·app typecheck PASS; 전체 check/clippy/build/E2E 미실행 |
| 2026-10-13 | 04 Task 1 별도 검토 보완 | 입력 FIN만으로 Drop 정리 | 독립 검토가 credit 대기 생산자 누수 RED 재현. StreamClose 제어로 채널 소유권 검증·stream generation 취소·등록/대기출력 정리, FIN half-close 보존. 뒤늦은 old ID 버퍼 재생성 RED도 active 등록/마지막 open 정리로 해결 | StreamClose 관련37개 영향 검사 PASS, 최종 client6·독립 drop 회귀1·실제 stream 통합3 PASS(6.024s, cgroup peak515760128B·swap0). 별도 재검토 PASS. 실제 Windows 검증은 아직 미실행 |
| 2026-10-13 | 04 Task 2 화면 스트림 | 초기 예제의 early 데이터 배열·FIN 폐기·직접 송신 | 별도 helper에 64KiB 페이지 coalescing·ID당256KiB/총4MiB/64ID early 제한, FIN 보존, ACK/onEnd 단발성, epoch 격리·StreamClose·orphan 정리·sendRaw 실패복원. onClosed 동기 재진입 call/open 거부 | 최초26 RED→GREEN, sync ACK 중 early 순서 RED [1,3,2]→수정. 관련 client/frame/stream74 PASS. 타입 인덱스 좁히기8곳 보정 후 app typecheck PASS, 독립 검토 PASS. 완료 Biome의 import 정렬 지적 보정 뒤 CI 통과. Task1 gen-ts/check 성공은 생성 입력 불변으로 재사용. 전체 build/E2E 미실행 |
| 2026-10-13 | C2 Windows 조기 스모크 사전 조회 | 기존 v1/prod 있으면 중단 | 읽기 전용 조회: v1 identifier 데이터·uninstall 등록 및 Ubuntu prod 데이터/unit/socket/bin 없음. DevboxSuite0.9.0·com.devbox.*와 기존 LocalAppData/Devbox는 존재하므로 보존, 전용 설치 경로 사용 예정. Ubuntu는 유일한 비내부 WSL 배포판이라 자동 선택. UIA RootElement 접근 가능, WebDriver 도구 없음 | 아직 설치·실행·제거 미실행이며 실기 PASS 아님. 실제 설치 직전 재확인하고 해당 Task2 SHA Windows CI 산출물만 사용. R1/R2는 S6까지 미실행 |
| 2026-10-13 | C2 Windows 조기 스모크 첫 시도 | Task2 SHA `9e21bc6a0f19020670723a400bc89cf1d6eb5681` | [Windows run38320568767](https://github.com/jihoon22-lee/devbox/actions/runs/38320568767) 출처/receipt·SHA256 `8ebf3651befa911a10d358a3f0c5fd8fa396fbe311aa7952a7cbe8f14aa2806d` 검증 후 전용 경로 설치·실행. Ubuntu 자동 선택·바이너리 배치·setup 수행, UIA에서 bridge Permission denied(os error13) 관측해 FAIL. projects/ping은 미실행 | wsl.exe root소유0600 FIFO의 /proc/self/fd 재열기 EACCES와 systemd EnvironmentFile 인용 경로 무시 발견. 앱 정상 닫기 후 이번 소유 prod/unit/socket/bin 및 설치본·identifier 데이터 정리, uninstall 등록 소멸 확인. v0.9/기존 데이터 보존. 수정 후 새 CI 설치본 재검증 예정 |
| 2026-10-13 | Windows 스모크 발견 결함 보완 | 기존 브리지 독립 OFD·인용 EnvironmentFile | EACCES+FIFO에 한해 소유 staging pipe/splice NONBLOCK으로 처리해 부모 flags 보존, EnvironmentFile raw 절대경로·% 이스케이프. 실패 회귀 뒤 bridge11·setup9 PASS, 각각 별도 검토 | 실제 Windows PowerShell→wsl.exe→수정 debug bridge→고유 임시 소켓 MAGIC/65,536B echo/exit0 확인. 설치본 PASS 대체 아님. 기존 서비스 변경 없이 fixture만 정리 |
| 2026-10-13 | C2 Windows 조기 스모크 재시도 | 수정 SHA `7eca14c5cd1809831c77f0d67ca77f5bdbfe0828` | [Windows run38322184013](https://github.com/jihoon22-lee/devbox/actions/runs/38322184013) 성공·repository/workflow/run/SHA receipt 대조, NSIS SHA256 `5c95c783779e069b85263c6b5e2320b42d46057232b67b7d2e9481f1f85abf44`. 전용 경로 설치 exit0, Ubuntu 자동 선택·Linux binary 배치(hash `5ab458e79c38aba31cd320270177c8c9cf2bdd53f1ca47c830faa831132cf25c`)·setup·prod active/정상 EnvironmentFiles 확인 | **PASS(실행한 S0b 범위)**: UIA 데몬 연결됨·프로젝트 빈 목록→소유 임시폴더 추가/선택, 실제 Windows wsl.exe bridge MAGIC/Welcome/ping 및 projects rev0→1. 앱 진단 정상 표시(기존 linger 꺼짐/inotify128 주의는 설정 변경 없이 보존). CloseMainWindow 수용·앱 종료, 추적한 Windows 자식11개와 Linux bridge 모두 소멸. user daemon은 설계대로 유지되다가 소유자 정리 단계에서 중지 |
| 2026-10-13 | C2 조기 스모크 정리·한계 | 소유 자원만 제거·미실행 구분 | NSIS 제거 exit0 및 설치 경로/등록 소멸, 이번 identifier Local/Roaming 제거. marker/hash와 사전 부재 대조 후 prod data/state/config·bin·unit/drop-in/enable링크 제거, 비활성 잔여socket은 연결거절·inode/uid 확인 후 제거. 소유 임시 프로젝트 제거. v0.9 DevboxSuite·catalog/com.devbox.* 보존, unit not-found/inactive | **미실행**: 다중 배포판 선택 UI(비내부 배포판 Ubuntu 하나라 자동 선택), S1 알림·트레이·전역단축키·분리창(후속 과제), R1/R2(S6). 첫 FAIL을 유지하며 이 SHA의 조기 스모크 PASS로 구분; S1 최종 SHA 수용을 대체하지 않음. 시작용 PowerShell harness 25초 timeout은 app PID/UIA로 실행 확인해 중복 실행 안 함; 일부 PowerShell interop Invalid argument 호출은 재시도 성공. 로그는 해당 CI 다운로드 디렉터리에 보존 |
| 2026-10-13 | 04 Task 3 tmux 세션 | forking unit·DB 뒤 await 발행·예제 프로필 누락 | S0a 근거 Type=exec/-D 독립 unit과 XDG/소유 conf 보호, bounded 명령·정확 세션 대상, 터미널 DB/프로필/CRUD/RPC·Projects.get·TS 연결. DB 실패 시 생성 세션 정리, commit callback 발행, 죽은 세션의 시작 명령 보존. 제어문자 cwd는 명시 거부하고 session ID는 전체 UUID 사용 | 실패 회귀 뒤 관련24개 PASS(tmux1/service5/readiness2/config2/sh_quote1/setup11/RPC재시작1/routes1), gen-ts/check/typecheck PASS. 빈 history의 화면 첫 줄 중복·Type=exec 소켓 준비 경쟁은 실제 fixture RED→GREEN. 독립 검토 PASS(현재 데몬 진입 경로); 전체 검사는 S1 끝에 수행 |
| 2026-10-13 | Task3 내부 호출 취소 검토 | RPC와 서비스 직접 호출의 수명 구분 | 독립 검토가 create_internal의 직접 future 중단+후속 DB insert 실패에서 미등록 tmux 잔존을 재현. 현재 terminal.create는 dedupe의 독립 TaskTracker·비취소 Ctx·shutdown drain으로 해당 경로를 차단하므로 Task3 blocking 아님 | Task8의 background 준비 호출은 동일한 실행 추적/완료 후 실패 정리를 보장하고 회귀를 추가해야 함. 현재 직접 서비스 호출까지 취소 안전하다고 주장하지 않음. 추측성 취소 기반을 Task3에 선행 추가하지 않음 |
| 2026-10-13 | 04 Task 4 PTY 스트림 | blocking PTY 입출력·stream ID만으로 resize·early return 정리 누락 | portable-pty의 소유 FD 복제/filedescriptor+AsyncFd로 독립 입력/출력, DB 소유 세션·connection/channel resize 검증, 읽기 전용 입력/크기 무시. 입력 FIN은 출력 유지, StreamClose/취소는 attach child kill/reap·등록 정리, tmux 세션 유지. async 준비 후 stream open, 자식 종료 후 남은 출력 drain | method 미구현 RED 뒤 관련25개 PASS, gen-ts/check/typecheck/Biome85 PASS. 정확한 0-credit·비어 있지 않은 history 조건에서 입력 진행과 PID 소멸 확인. raw row80 개수는 tmux 정상 redraw로 중복되므로 선채움 prefix가 visible row80을 제외하는 의미 검사로 보정. 독립 검토 PASS, 정책 지적은 아래 행에서 보완 |
| 2026-10-13 | Task4 독립 검토 오류 경계 보완 | PTY/Tmux raw 원인을 도메인 오류 detail로 반환 | 01 §5.3에 맞춰 내부 PTY I/O·tmux Spawn/명령 실패는 Fail::Internal→진단 ID로 반환. private marker 노출 Router RED→GREEN, raw reason 변형/메시지 제거·타입 재생성. 정상 Linux PTY EIO·스트림 종료·전체 취소는 오류로 기록하지 않음 | 정책 수정 뒤 영향15개 PASS(terminal lib3/attach2/service5/CLI attach4/session1), gen-ts/check/typecheck/Biome85 PASS. 실패를 기록하고 영향 범위만 재검증. 독립 재검토 PASS |
| 2026-10-13 | 04 Task 5 Git 조회 | 단순 문자열/공백 파싱·오류 시 빈 값·빈 diff의 신규 파일 fallback | strict UTF-8/NUL 파싱·literal pathspec·검증한 commit OID, nonrepo/detached/unborn 구분, 저장소 루트 기준 경로 일치. tracked 무변경은 빈 패치, untracked symlink는 링크 문자열만, 충돌 index 상태를 변경 목록에 반영. subprocess 출력 각8MiB/30초 제한·즉시 kill/reap, 저장소 경로 환경만 제외하며 인증 설정 유지 | API 부재 RED 및 충돌 M 오분류 RED 뒤 관련9개 PASS(runner2/query7), diff 검사 PASS. 내부 OS 오류는 원인을 로그에 두고 GitError::Internal로 분류; 후속 RPC caller에서 Fail::Internal 매핑 필요. 독립 검토 지적은 아래 행에서 보완 |
| 2026-10-13 | Task5 독립 검토 조합·조회 부작용 보완 | staged 삭제 후 같은 경로 재생성 시 목록/패치 불일치 | 변경 재생성은 Modified+1/-1와 원문/새 내용 패치로 일치, 동일 내용은 net content diff 없음(status는 staged/untracked 변화 보존). 소유 임시 index의 read-tree→diff만 사용하고 실제 index/refs/objects는 보존. split-index/sparse 설정을 해당 경로에서 끄며 실제 index bytes/저장소 파일 목록 불변 회귀 | 조합 RED→GREEN 뒤10개 PASS. 재검토에서 read-tree의 post-index-change hook 실행을 marker RED로 재현, 해당 임시 비교 호출에만 소유 빈 hooksPath를 적용. 영향 recreated 회귀2개 PASS, 일반 mutation hook 정책 미변경. 독립 재검토 PASS |

| 2026-10-13 | 04 Task 6 Git 변경 | worktree·병합·rebase·push/PR 기본 래퍼 | canonical 등록/common-dir·primary/locked/별칭 보호, 원본 OID 고정·기준 branch/HEAD 재확인 후 소유 병합만 복구, 실제 gh 대신 소유 fake gh·로컬 bare remote로 검증. PR 본문은 private 임시 파일과 --body-file 사용. Unix 명령 전용 process group으로 timeout/취소 자식 정리 | 초기 Git 전체23개 PASS(runner4/mutation10/query9). 별도 검토에서 hook 실패 뒤 부분 생성과 @{-1} 브랜치 별칭 삭제를 재현; 부분 자원·원인을 typed error로 반환하며 hook 변경 보존, literal ref 검증 후 mutation12 PASS·검토자 두 회귀 각1 PASS·최종 검토 PASS. Task7에서 중첩 내부 원인 비노출, Task8에서 부분 자원 reconcile 필요. Windows 자식 수명 실기는 미실행 |

| 2026-10-13 | 04 Task 7 에이전트 기반 | 모델·상태 전이·JSON 저장·프로필·설정 및 Git 오류 변환 | typed model/state와 폐쇄 시각 기준 목록·upsert/index, core의 중립 TOML profile 설정 및 도메인 strict 파싱. 외부 타입에 대한 From 구현은 Rust orphan 규칙상 불가하여 git_failure 함수로 대체; raw/중첩 원인 비노출. branch_for/worktree_dir도 Result로 공통 slug 검증 후 같은 이름 보존 | API 부재 RED 뒤 agents12 PASS, 추가 상태 범위4 PASS·설정11 PASS. 별도 검토의 fix--login/fix-login 폴더 충돌 RED 뒤 naming7 PASS, 최종 검토 PASS. Task8은 변경한 helper Result에 ? 적용. 커밋 전 Biome85 PASS |

| 2026-10-13 | 04 Task 8 생성·hook·수명 | preparing 저장 뒤 detached 준비·단순 hook 상태 갱신 | DB 커밋 callback의 추적 토큰으로 취소된 create waiter도 준비 완료/실패까지 drain, 동시 branch/path/port 예약, 최신 행 재조회 갱신. Git 부분 생성의 소유 상태를 별도 저장·재시작 대조하며 hook 파일 보존. CLI stdin/connect/RPC 전체3초 제한, no-ID 즉시 no-op, 준비 실패 뒤 셸 유지. 프로젝트 설정 strict table·copy 경로/권한 보존 | 실제 CLI 부재 RED 뒤 관련29개 PASS(CLI3/agent2/Codex RPC1/daemon21/terminal1/route1), gen-ts/check/typecheck PASS. agents 초기35·최종 자원4 PASS. 별도 검토의 이전 날짜 known-thread 재탐색·metadata/무효 hook의 attention 덮어쓰기 회귀 RED 뒤 agents40 PASS·재검토 PASS |
| 2026-10-13 | 04 Task 8 Codex 실제 계약 | metadata source·현재 turn gate의 fixture 검증 | 정확한 cli/user·workdir·launch·현재 turn을 검증하고 private binding 저장. 같은 DB transaction에서 완료 후보의 최신 binding을 재검사, 동일 binding은 쓰기·이벤트 생략. missing/ambiguous는 bounded retry 뒤 typed diagnostic, 상태 유지. 주 세션이 아닌 객체형 source는 먼저 제외하며 숫자형 started_at 대신 이벤트 봉투 timestamp 사용 | 실제 Codex 두 번은 PONG 출력 후에도 Running/sessionId 없음으로 TIMEOUT(75초): 첫 숫자 started_at, 다음 보조 세션 객체 source가 원인. 각각 synthetic RED 후 수정, agents40 및 tracker7/correlation5·최종 Codex RPC1 PASS. 별도 재검토 PASS. 원시 실패 근거도 보존 |
| 2026-10-13 | 04 Task 8 실제 CLI 수용 | CLI 실행 인자 hook 주입·사용자 설정 보존 | 이미 신뢰된 main cwd, 프로젝트 설정 파일 없음·worktree=false, 전용 test 인스턴스/XDG/tmux에서 도구 사용 없는 PONG만 요청. Claude 2.1.296 PASS(생성→idle 약3.284초), Codex 0.162.1 수정 뒤 PASS(약8.094초), 정확한 primary thread/turn metadata와 binding 대조. 이 시간은 SC3 hook→화면 측정값이 아님 | 근거 Git common-dir `devbox-verification/s1-task8-hooks-aa0z3vxv`(Claude PASS/Codex 첫 실패), `s1-task8-hooks-15667yzo`(두 번째 실패), `s1-task8-hooks-8imklb7_`(Codex PASS·binary SHA256 13aaf6836dcf1b2e625dc90d8f8179c13f62cc6d1ecc089b8b8fca6ef0d62586). config SHA/mtime·main status 불변, 소유 CLI PID/starttime·tmux·daemon 종료·runtime 정리 확인. 첫 stale runtime은 소유 확인 후 후속 정리 기록. 새 fixture cwd 신뢰 승인·전역 설정 변경 없음; Windows 실기는 최종 Task24 별도 |
| 2026-10-13 | 04 Task 9 작업 행동·검토 조회 | 행동·조회 RPC 없음, Task 8의 waiting→send 시험 유보 | 지시·재개·검토·rebase/abort·병합·PR·폐기·테스트·정리·변경/patch/commits 15개 API 연결. 확인 OID 기반 실제 source 고정, 생성 TS에서도 행동별 필수 확인값 보장. 재개 직후 hook 세션 정보와 최신 launch binding 보존, 닫힌 커밋 수·OID·시각 고정 | 최초 agents.changes method_not_found RED. CLI actions7/safety7/cleanup1·기존 agents2(waiting→send→idle) PASS, agents 단위46·Git mutate13/lib4·terminal service5/lib3 PASS. xtask 확인 계약1·gen-ts PASS. 로컬 bare 원격+가짜 gh로 PR 성공/실패 검증; 실제 외부 PR 생성 아님 |
| 2026-10-13 | 04 Task 9 소유 자원·부분 정리 | 이름 패턴 정리와 닫기 후 오류 구분 부족 | 저장된 소유 상태·Git 등록·닫을 당시 OID·활성 충돌 재검사. dirty/locked/미병합/부분 생성/호스트/재사용 자원 보존, 사라진 worktree의 소유 브랜치만 별도 정리 후 소유 해제. cleanupError로 병합·PR 성공과 정리 실패를 구분. inline 세션 끝내기는 원본 checkout·브랜치·수정/미추적 파일 보존, teardown 생략 | 재사용 ref·외부 worktree·symlink·teardown dirty 보존·병합 충돌 복구·소유 rebase abort·60초 teardown 제한의 자식 process group 정리 회귀 PASS. 전체 check/clippy/build/E2E·Windows 최종 실기·실제 scope 실행은 이 과제에서 반복하지 않음 |
| 2026-10-13 | 04 Task 9 별도 검토 보완 | 이미 사라진 scope 정지 실패·inline 최초 브랜치 강제 | 독립 검토에서 두 실패 재현: missing scope는 정확한 unit LoadState=not-found만 허용, loaded/버스 실패는 계속 거절. inline은 현재 HEAD·확인한 미병합 수를 검증하고 브랜치 변경 자체는 허용. mutation 취소 우려는 기존 Dedupe의 추적 실행 보호를 확인해 철회 | fake systemctl 4모드(부재 exit0/1·loaded 실패·버스 실패) 단위1 PASS, stale→fresh inline 확인 포함 actions8 PASS. 실제 서비스 변경 없음. 독립 재검토·diff check·완료 Biome CI 85 files PASS. 전체 S1 검증은 Task24에 수행 |

## 6. 세션 기록

세션이 끝날 때 한 줄을 덧붙인다. 오래된 줄은 지우지 않는다.

| 날짜 | 하네스 | 한 일 | 남긴 상태(브랜치·커밋·다음 단계) |
|---|---|---|---|
| 2026-10-08 | Claude Code | 세 계획(rebuild·CV·AR)을 이 폴더로 통합·검수(08 §8·§9), 원본 세 폴더 삭제, P0 | `docs/v1-plan`에 커밋·push(PR·CI 없음). 원본 체크아웃의 추적 안 된 사본은 지움. 다음: S0a |
| 2026-10-12 | Codex | 사용자 요청에 따라 계획 검토·착수 환경 확인만 수행. 아래 §7에 준비 결과 기록 | `docs/v1-plan@4aa88e62` 기준. S0a 미착수, 다음: Task 1의 측정 도구 보완 후 실험 |
| 2026-10-12 | Codex | S0a Task 1–7 측정·별도 검토, Task 8 결과·ADR 초안·source 계약 보정·소유 자원 정리 | `docs/v1-plan` 문서 통합·Task 8 최종 검토 통과. 자동 부분 완료, R1/R2 유보. 다음: 문서 커밋/보고 뒤 S0b Task 1(미착수) |
| 2026-10-13 | Codex | S0 최종 CI 통과·PR #638 squash 머지, main 미푸시 2개 처리·S0 작업 공간 정리, S1 준비 | `v1/s1-agents-terminal`은 `820576c9`에서 분기. Task 1 구현 미착수, 다음: 스트림·공정 송신 실패 회귀. 중간 사용자 테스트 요청 없음 |
| 2026-10-13 | Codex | PR #638 머지 후 자동 리뷰 1건을 검증·수정, PR #639 최종 CI와 코드·보안 리뷰 완료 후 머지 | main `fb6bc69d`와 S1 준비 브랜치 동기화. S1 준비 기록은 이미 후속 PR에 포함돼 중복 커밋 재적용 없이 최신 main으로 이동. 다음: S1 Task 1 |
| 2026-10-13 | Codex | S0 검토 A/B 재확인·C 사용자 결정 반영, 재추정, glib 경보 dismiss, 통합된 docs/v1-plan 정리. 독립 문서 재검토·Biome 통과 | `v1/s1-agents-terminal`, 기존05c13913 보존. 문서 변경은 로컬 커밋, push/PR 없음. 다음 Task1 credit·Outbox(기존 flush ACK/세대/dedupe 보존), Windows 스모크는 Task2 뒤 조기 시도 |
| 2026-10-13 | Codex | S1 Task 1 credit·공정 송신 구현, 별도 검토의 스트림 폐기/late buffer 누수 수정 및 재검토 통과 | `v1/s1-agents-terminal` 로컬 Task 1 완료. 직접 회귀·TS 생성/typecheck·Biome 검증 후 과제 커밋, 다음 Task 2·조기 Windows 스모크. S1 전체 검증/PR은 아직 |
| 2026-10-13 | Codex | S1 Task 2 화면 스트림·수명 회귀 구현 및 독립 검토 통과 | Task1 `f1f4e445` 뒤 Task2 과제 커밋. 묶음 F 로컬 완료, 다음 승인된 조기 Windows 스모크. 일반 묶음 push/PR 없음, 스모크용 SHA push·workflow만 예외 |
| 2026-10-13 | Codex | Task2 뒤 승인된 Windows 조기 스모크에서 bridge/setup 결함 발견·회귀/독립검토 후 수정. 새 CI 설치본으로 실행한 S0b 범위 PASS, 소유 자원 정리 | `v1/s1-agents-terminal` Task1 `f1f4e445`, Task2 `9e21bc6a`, 수정/실기 source `7eca14c5`(원격 일치), 후속 실기 기록 로컬 커밋. 다음 Task3 tmux 미착수(읽기 전용 준비만). S1 전체 검사·브랜치 전체 검토·최종 Windows 실기·PR/머지 미실행 |

## 7. 착수 준비 검토

아래는 실행 지시 전 준비 검토 시점의 기록이다. 그때는 S0a 실험·제품 구현·설정 변경·PR·CI를 실행하지 않았다. 현재 실행 상태는 §1과 09-s0a-results.md를 따른다. 전체 로드맵과 단계별 과제 구성을 확인하고, S0a 8개 과제와 연결된 설계를 우선 검토했다. S0b·S1의 모든 예제 코드를 컴파일 검증한 것은 아니다.

### 확인한 상태

- `git fetch origin` 후 `docs/v1-plan`과 원격은 `4aa88e62`로 일치한다. 기존 worktree `../devbox-wt/docs-v1-plan`을 사용한다. 열린 PR과 원격 `v1/*` 브랜치는 없다.
- 원본 `main`은 clean이며 원격보다 2개 커밋 앞선 상태다. §2에 기록된 두 커밋은 보존했다. 이 결정과 저장소 커밋 이메일 변경은 S0a 준비를 막지 않는다.
- Node 24.18.0, pnpm 9.0.0, Rust/Cargo 1.98.1, musl target, tmux 3.6, systemd 259, jq, actionlint, gh 인증을 확인했다. `systemctl --user is-system-running`은 `running`이다.
- Claude Code 2.1.296, codex-cli 0.162.1이다. 계획 당시 버전과 달라 실제 hook 실험에서 계약을 다시 확인해야 한다. 이번에는 버전만 확인했다.
- Windows `cmd.exe`, `wsl.exe`, PowerShell 5.1은 절대 경로 호출에 성공했다. 현재 셸 PATH에는 없고 `WSL_DISTRO_NAME`도 비어 있으므로 실험 도구는 배포판 `Ubuntu`와 실행 파일 경로를 명시한다. systemd 서비스 내부 interop·DPAPI는 아직 측정하지 않았다.
- cargo-sweep은 없다. S0a에는 필요 없으며 S0b 정리 도구 준비 시 처리한다. 여유 디스크 약 678GiB, 메모리 available 약 8.1GiB, swap 사용 약 4.8GiB를 확인했다. 무거운 검증은 공통 자원 제한과 worktree 간 잠금을 따른다.

### 실행 전에 반영할 사항

| 위치 | 확인한 사항 | 처리 |
|---|---|---|
| 02 Task 1·4 | Task 4는 브리지 p95를 요구하지만 Task 1은 p50·p99·max만 출력한다 | 측정 도구에 p95를 추가한다. PTY와 브리지 백분위 합은 예비 판단으로 구분하고, 최종 SC10은 S1의 전체 경로 측정으로 판정한다 |
| 02 Task 1 | `RoundTrip`은 큰 payload를 모두 쓴 다음 응답을 읽는다. 에코 경로의 양방향 버퍼가 차면 서로 기다릴 위험이 있고 `ReadExact`에 시간 제한도 없다 | 읽기·쓰기를 동시에 진행하고 전체 측정 시간 제한과 자식 프로세스 정리를 둔다. 실제 교착을 재현한 결과는 아직 없다 |
| 02 Task 4 Step 6 | PTY 응답 시간 초과도 정상 sample로 넣고 다음 측정으로 넘어간다 | 시간 초과·EOF는 별도 실패로 기록하고 합격 판정에서 제외하지 않는다 |
| 02 Task 8 Step 4 | 대상 없는 `systemctl --user reset-failed`는 실험 외 unit의 실패 상태도 초기화한다 | 이번 실행에서 생성·소유한 unit만 지정해 정리한다. 고정 `s0a` 이름·경로가 기존에 있으면 덮어쓰거나 일괄 삭제하지 않는다 |
| 00 §3·§3.6, 01 §11, 03·03b의 Git/검증 절차 | 계획의 rebase 머지·묶음별 전체 검사/push·Cargo jobs 4 또는 트랙별 3은 이번 사용자 제공 AGENTS 및 CONVENTIONS의 squash·PR 끝 통합 검증/push·공통 자원 제한과 충돌한다 | 이번 세션의 명시 지침을 우선한다. S0b에서 새 운영 규약과 CI를 만들 때 해당 절차를 함께 정합화한다. 계획의 과거 승인 문구만으로 현재 지침을 덮어쓰지 않는다 |
| 03b Task 24 | 실제 main 보호 규칙은 기존 필수 검사 3개이며 `ci-ok` 전환은 아직 이루어지지 않았다 | S0b 최종 수용 시 새 CI와 현재 지침의 Windows 검증 요건을 함께 확인한다. 이번에는 보호 규칙을 변경하지 않았다 |

다음 실행 순서는 S0a 측정 도구 보완 → 브리지 → systemd/socket activation·실행 수명 → tmux/PTY → systemd 내부 interop·DPAPI → hook 실험 → 결과·ADR 초안·소유 자원 정리다. S0a 합격 뒤에만 S0b로 간다. 이 준비 검토 당시의 S0b 사용자 확인 gate는 2026-10-13 결정으로 대체하며, 설계 변경이 필요한 실패는 별도로 보고한다.
