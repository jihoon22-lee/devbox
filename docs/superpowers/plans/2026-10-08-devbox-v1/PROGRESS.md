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
| 단계 | S0b PR #638 열림, S0a 자동 부분 완료·R1/R2는 S0 확인 유보 |
| 다음 할 일 | PR #638 자동 리뷰의 연결 예외 3건 보완 → 최종 CI/설치 파일 → 사용자 S0 확인 (dd51f2e1의 필수 CI·native 16 tests/NSIS/self-check 통과, 아직 미머지) |
| 동시 진행 | S2 뒤 한 세션이 두 트랙을 동시에(00-roadmap §3.6). 그때는 아래에 "트랙 1(S3→S4) 위치"·"트랙 2(S5) 위치" 두 줄을 쓴다 |
| 작업 중인 브랜치·worktree | `v1/s0b-skeleton` · `../devbox-wt/v1-s0b-skeleton` — S0a 결과 `58fda4d7`에서 분기, 현재 계획의 원본 |
| 마지막 갱신 | 2026-10-13 · Codex 실행 세션 |

## 2. 막힌 것 · 사용자 결정 대기

| 날짜 | 무엇 | 누구 결정 | 상태 |
|---|---|---|---|
| 2026-10-08 | P0 진행 방식 | 사용자 | 해결: PR·CI 없이 `docs/v1-plan`에 커밋·push(`aa5c6785`), main에는 S0b PR과 함께 |
| 2026-10-08 | PR 47개 → 하위 프로젝트당 1개(7개)로 줄임 | 사용자 | 해결(00-roadmap §3) |
| 2026-10-08 | S2 뒤 S3·S5를 동시에 진행할지 | 사용자 | 해결: 동시 진행. 한 세션이 하위 에이전트로 트랙 1(S3→S4)·트랙 2(S5)를 함께 돌림, 사용자가 세션을 더 띄우지 않음(00-roadmap §3.6) |
| 2026-10-09 | 사용자 확인 방식 | 사용자 | 해결: S0 끝(S0b) 한 번만 기다리고, 그 뒤는 v1.0.0 공개 직전과 §3.2 상황에서만 묻는다. 확인 목록은 머지 뒤 보낸다(00-roadmap §2.2) |
| 2026-10-08 | 이 PC의 로컬 main에 원격에 없는 커밋 2개(`b2a508b6` source-map-js 갱신, `1b84aa9d` 완료 계획 정리, 둘 다 2026-10-07). `docs/v1-plan`은 원격 main(`72ff50c7`)에서 갈라졌다. 두 커밋의 내용은 S0b PR(옛 코드·문서 전부 삭제)로 의미가 없어진다. S0b PR 머지 뒤 로컬 main을 원격에 맞출지(두 커밋 버림), 그 전에 PR로 올릴지 | 사용자 | 대기 |
| 2026-10-08 | 저장소 `user.email`이 개인 Gmail이라 명령줄 push가 거절된다(GitHub 이메일 비공개). P0 커밋은 noreply 주소로 다시 써서 push했다. 저장소 설정을 `188744452+jihoon22-lee@users.noreply.github.com`으로 바꿀지 | 사용자 | 대기(그 전까지 커밋마다 `-c user.email=…`, 00 §3.3) |

## 3. 하위 프로젝트

| S | 계획 문서 | 상태 | 시작 | 완료 | 사용자(§2.2) |
|---|---|---|---|---|---|
| P0 | 00-roadmap §2 | 완료 | 2026-10-08 | 2026-10-08 | — |
| S0a | 02-s0a-spike | 자동 부분 완료·수동 유보 | 2026-10-12 | 2026-10-12 | 결과 보고만 |
| S0b | 03-s0b-skeleton · 03b-s0b-app | PR 열림 | 2026-10-12 | | **S0 확인 8개(기다림)** |
| S1 | 04-s1-agents-terminal · 04b-s1-app | 대기 | | | 확인 목록 10개(보냄) |
| S2 | 05-s2-runs | 대기 | | | 확인 목록 10개(보냄)·전환 안내 |
| S3 | 06-s3-knowledge | 대기 | | | 확인 목록 10개(보냄) |
| S4 | 07-s4-s6(→ 07a) | 대기 | | | 확인 목록 8개(보냄) |
| S5 | 07-s4-s6(→ 07b) · S2 뒤 S3와 동시(트랙 2) | 대기 | | | 확인 목록 6개(보냄) |
| S6 | 07-s4-s6(→ 07c) · S4·S5 뒤 | 대기 | | | **v1.0.0 공개 직전 확인(기다림)** |

## 4. PR (모두 7개)

하위 프로젝트 하나 = 브랜치 하나 = PR 하나다(00-roadmap §3). 머지하면 PR 번호와 머지 날짜를 적는다.

| PR | 브랜치 | 범위 | 상태 | PR 번호 | 머지 |
|---|---|---|---|---|---|
| S0b | `v1/s0b-skeleton` | S0b Task 1–24 (+ 계획 폴더) | PR 열림 | #638 | |
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
| S1 | F | Task 1–2 | `v1/s1-agents-terminal` | 대기 | | | |
| S1 | G | Task 3–4 |  | 대기 | | | |
| S1 | H | Task 5–6 |  | 대기 | | | |
| S1 | I | Task 7–10 |  | 대기 | | | |
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
| 2026-10-12 | 02 Task 7 · 03b Task 24 확인 7 | 트레이에서 30분 유휴 | 설치 창 열고 연결 유지 30분; R1 확인 7/R2 확인 4·8 유보, 미실행 | 트레이는 S1 Task 23 범위. 조기 구현하지 않고 S0 확인의 connected-client 목적 유지 |
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
| 2026-10-13 | 03·03b 운영 절차 | 묶음별 전체 검사/push·ci-ok 단일 보호 전환·rebase | 현재 사용자 규약대로 과제별 좁은 검사, Task 24 통합 검증/push, 기존 필수 CI 3개 유지·S0 확인 뒤 squash | 수용·머지 전 완료로 기록하지 않도록 순서도 정리. 구현자와 별도 검토 완료는 로컬 진행이며 PR/Windows 실기 통과와 구분 |
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

## 6. 세션 기록

세션이 끝날 때 한 줄을 덧붙인다. 오래된 줄은 지우지 않는다.

| 날짜 | 하네스 | 한 일 | 남긴 상태(브랜치·커밋·다음 단계) |
|---|---|---|---|
| 2026-10-08 | Claude Code | 세 계획(rebuild·CV·AR)을 이 폴더로 통합·검수(08 §8·§9), 원본 세 폴더 삭제, P0 | `docs/v1-plan`에 커밋·push(PR·CI 없음). 원본 체크아웃의 추적 안 된 사본은 지움. 다음: S0a |
| 2026-10-12 | Codex | 사용자 요청에 따라 계획 검토·착수 환경 확인만 수행. 아래 §7에 준비 결과 기록 | `docs/v1-plan@4aa88e62` 기준. S0a 미착수, 다음: Task 1의 측정 도구 보완 후 실험 |
| 2026-10-12 | Codex | S0a Task 1–7 측정·별도 검토, Task 8 결과·ADR 초안·source 계약 보정·소유 자원 정리 | `docs/v1-plan` 문서 통합·Task 8 최종 검토 통과. 자동 부분 완료, R1/R2 유보. 다음: 문서 커밋/보고 뒤 S0b Task 1(미착수) |

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

다음 실행 순서는 S0a 측정 도구 보완 → 브리지 → systemd/socket activation·실행 수명 → tmux/PTY → systemd 내부 interop·DPAPI → hook 실험 → 결과·ADR 초안·소유 자원 정리다. S0a 합격 뒤에만 S0b로 간다. 사용자 확인 시점은 계획의 S0b 끝과 공개 직전을 유지하되, 설계 변경이 필요한 실패는 별도로 보고한다.
