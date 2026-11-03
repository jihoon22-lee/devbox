# 진행 원장

devbox의 진행 상태를 적는 **유일한 원장**이다. 계획 문서의 체크박스와 상태 줄은 보조다. 세션을 시작하고 끝낼 때 [CONVENTIONS §11](../../../../CONVENTIONS.md#11-기록과-인계)대로 이 파일을 읽고 고친다.

## 0. 읽는 법

- 이 파일은 지금 상태만 적고 **200줄을 넘기지 않는다**(사용자 결정 4, S8-2). 줄이기 전 전문(S0–S8-0의 결정 표·과제 기록·계획 대비 편차·세션 기록)과 끝난 계획은 [docs/history.md](../../../history.md)의 고정 링크로 본다. 지난 결과·근거 경로·Ruling은 거기서 찾는다.
- 이 파일은 PR과 함께 main에 들어간다. 작업 중인 브랜치가 있으면 **그 브랜치의 이 파일이 더 최신**이다(CONVENTIONS §11 세션 시작 2).
- 판정은 PASS·FAIL·NOT_RUN이다. 실행하지 않은 확인은 PASS로 쓰지 않는다. 결과는 그 결과를 낸 source SHA의 것이다.
- 날짜는 `YYYY-MM-DD`, 시각은 KST다. 근거 경로 `devbox-verification/…`은 이 PC에만 있다(§6).
- **세션 규칙:** 세션이 끝날 때 §7에 한 줄을 더하고 최근 10줄만 남긴다. 밀려난 줄은 이 파일의 git 이력에 남는다. 이 규칙이 "오래된 줄은 지우지 않는다"를 대신한다(근거: 결정 4).

## 1. 현재 위치

- **2026-11-04 · S8(1.0.0) 작업 중.** 브랜치 `v1/s8-release`, worktree `/home/jihoon/projects/devbox-wt/v1-s8-release`, 계획 [10-s7-s8-release](10-s7-s8-release.md#s8--100). 2026-11-03 origin/main `842fa82e`에서 만들었고 push·PR·태그 전이다.
- 이 브랜치의 커밋: S8-1 `a6f75a68`(버전 `1.0.0`), S8-1b `eb08e39b`(Dependabot 경고 7건: zip 9.0.5·seroval 1.7.7, Ruling 99), S8-0 `cfa89f0a`(S7-14–S7-16 결과 기록), S8-2a `71b42eb4`(끝난 계획 보관과 [docs/history.md](../../../history.md))와 이 원장 재구성.
- S7(rc.2 보강)은 끝났다. PR #651을 2026-11-03 12:45 KST에 squash 머지했고(`842fa82e`, 검증한 head `400a737f`와 tree 동일) main CI가 SUCCESS다. main `842fa82e`의 주석 태그 `v1.0.0-rc.2`로 prerelease를 공개했다(release run `41604456654`, 17:57 KST, asset SHA-256 `c0545772…`). `releases/latest`는 `v0.9.0`이고 공식 `v1.0.0` 태그·Release는 없다.
- 다음: S8-2b(guide·개발 문서 합치기·ADR 상태) → S8-3 검사·후보(`pnpm check` 한 번, 1.0.0 후보 accept 설치 스모크, 릴리스 노트 초안. Windows 행은 interop 고장과 Q1 때문에 S8-3에서 판정한다. CI에서 `linux-bin`의 `Compiling zip v9.0.5`와 `nix 0.31.3` 없음, clippy·`support_bundle` 15·고지 시험을 본다) → S8-4 PR·리뷰·머지(Dependabot PR #652는 닫는다) → S8-5 1.0.0 공개(**별도 명시 승인**, §3을 제시) → S8-6 공개 기록.
- S7 worktree·브랜치·`/tmp` 근거는 정리했다. 공유 target `~/.cache/targets/devbox`는 2026-11-03에 151G → 107G다(`cargo sweep --time 3` 뒤 `debug/incremental/` 47G를 비움, CONVENTIONS §5).

## 2. 열린 결정

사용자만 정할 수 있는 것이다. 답이 오면 여기서 지우고 §3·§4에 반영한다. 승인 원문과 닫힌 결정은 이전 전문 §2([docs/history.md](../../../history.md))에 있다.

- **① WSL→Windows interop 고장(W1):** `cmd.exe /c ver`가 `UtilAcceptVsock:273: accept4 failed 110`으로 실패한다(VM 전체, 2026-10-21 S2 설치 수용 때와 같은 증상). 마지막 확인은 2026-11-03 18:39 KST이고 WSL boot는 2026-10-22 22:54 그대로다. 사용자 지시(2026-11-02): WSL 재시작을 요청하지 않는다(Q0 철회). Windows 실기는 WSL이 스스로 다시 시작된 뒤에만 한다. 그래서 Windows 행이 NOT_RUN이고 I1은 공개 승인 때 미검증으로 알린다(§3). W1 자료가 나오면 Fix C의 "사용자 확인 뒤 남은 프로세스 끝내기"를 사용자가 정한다.
- **② S7-14 Q1·Q2:** claude·codex의 폴더 신뢰 수락을 승인하지 않았다. 수락하면 `~/.claude.json`·`~/.codex/config.toml`에 항목이 남고 정리가 지우지 않는다. 두 도구의 turn·승인·재개 행이 NOT_RUN이다.
- **③ S7-14 Q6:** opencode는 "This API key has expired…", pi는 모델 "Muse Spark 1.3 Free"의 "Model is unavailable"로 멈췄다(도구 자신의 인증·모델 오류). 에이전트 작업은 셸의 API 키 변수를 받지 않는다(README.md:44). 사용자가 본인 터미널에서 같은 프롬프트로 재현하는지 확인하기 전에는 원인을 단정하지 않는다(Ruling 87 보정).
- **S5 수용 3·6(2026-10-21, S5-9 수용에만 걸림):** 외부 서비스의 웹훅 수신(LAN 노출·포트 전달·방화벽)과 빠진 도구의 실제 설치(`sudo apt`·`npm -g`·`winget`). 기본안만 확인하고 실제 실행은 사용자 승인 뒤에만 한다(07b).
- **다가오는 결정 · S8-5 `v1.0.0` 공개:** 그 시점의 별도 명시 승인 뒤에만 태그를 push하고 Release를 낸다. 2026-11-03의 rc.2 공개 승인과 S8 의존성 포함 승인은 공식 `v1.0.0` 승인이 아니다.

## 3. 1.0.0 공개 게이트

S8-5에서 사용자에게 제시한다(계획 10 S8-5). 기록된 판정만 옮기고 올리지 않는다. 사용자는 목록을 모두 직접 실행하지 않고도 승인할 수 있고, 사용자 몫은 결과가 없으면 NOT_RUN이다([07c S6-6c](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md#s6-6c-공개-직전-단-한-번의-명시-승인)). 실측이 미달이면 원인·최소 수정/재측정 또는 명시적 미달·다음 patch 계획을 적고, 근거 없는 목표 완화·측정 생략은 하지 않는다([07c §14 SC 원장](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md#sc별-판정-원장)). `40d97a83-…`·`400a737f-…`·`19cff21e-…`는 수용 harness 실행 ID다(§6).

| 항목 | 판정 | 근거 또는 이유 |
|---|---|---|
| 1 새 설치 첫 화면 | NOT_RUN | S7-14의 S6 묶음 A2를 interop 고장으로 실행하지 못했다(§2 ①). 이전 source: S6-6b A2 PASS(launch, `19cff21e-d2254acc`) |
| 2 agent 전 과정 | NOT_RUN | S7-14 H1–H4: claude·codex는 Q1·Q2 미승인, opencode·pi는 도구 자신의 오류(Q6). 실행 `40d97a83-a4e9f78c` |
| 3 service/log | PASS | S7-14 SV1(`[[service]]` 시작 → 로그 → 정지, 데몬 RPC): `40d97a83-a4e9f78c`, 최종 head 재확인 `400a737f-11d1bb99`. rc.2 source의 WSL accept 결과이고 Windows 화면 확인은 아니다 |
| 4 note 작성 | PASS | S7-14 NT1(`devbox note add` → 검색): 같은 두 실행. 범위는 3과 같다 |
| 5 v0.9 제거 뒤 정상 | NOT_RUN | 사용자 몫. S7-16에 부탁했고 S8-0 기록 때까지 받은 결과가 없다. S8-5 전에 오면 기록하고 결함이면 S8에서 고친다(07c, Ruling 97) |
| 6 Windows 배율 150%/200% 가독성 | NOT_RUN | 사용자 몫(5와 같다) |
| 7 키보드만으로 palette → 작업 → 확인 → 오류 재시도 | NOT_RUN | 사용자 몫(5와 같다) |
| 8 API 응답 일일 기록 append | NOT_RUN | 사용자 몫(5와 같다) |
| 9 editor JSON 선택 → 변환 도구 | NOT_RUN | 사용자 몫(5와 같다) |
| 10 R1: 30분 idle 연결·세션 유지 | NOT_RUN | 사용자 몫(5와 같다) |
| 11 R2: WSL shutdown 뒤 reconnect·소요 시간 | NOT_RUN | 사용자 지시(WSL 재시작을 요청하지 않는다)에 따라 요청하지 않았다(Ruling 97) |
| 12 방해 금지 알림 센터 기록·해제 뒤 popup/taskbar | NOT_RUN | 사용자 몫(5와 같다) |
| 13 비내부 WSL 배포판 2개 이상 선택 화면 | NOT_RUN | S7-14 DS1을 interop 고장으로 실행하지 못했다. 배포판이 하나뿐이면 해당 없음(07c) |
| I1 Windows 사본 reader가 고아 자손을 놓침 **[차단]** | NOT_RUN | 결함은 S6-6b G1(`19cff21e`)이 실제 Windows에서 CONFIRMED했다. S7-10이 고쳤다(`681a7b82`·`64d14394`·`68bb9e82`, S7-13 Fix C 보강). 실제 Windows 확인 W1은 S7-14 NOT_RUN(§2 ①). 결정 2: 확인하지 못하면 1.0.0 공개를 막는다. 공개 승인 때 미검증으로 알린다(Ruling 90). rc.2는 이 상태로 공개했다. 남은 위험: 같은 로그온 세션에서 holder만 강제 종료되고 bootstrap도 끝났는데 고아가 돌면 reader가 일찍 닫힌다 |
| SC1 흔적 없는 전용 Windows 계정에서 setup → 첫 데이터 | NOT_RUN | 전용 계정·독립 환경이 없다(S6-6b `19cff21e-d2254acc` `acceptance.json` notRun). 기존 사용자 자료를 지워 흉내 내지 않는다 |
| SC2 단축키 → palette ≤300 ms, cold → 첫 데이터 WSL running ≤3 s·stopped ≤10 s | FAIL(미달) | cold → 첫 데이터 raw 9036 ms(`19cff21e-85014e8e`)·6093 ms(`19cff21e-d2254acc`): 각 1표본, WSL running, 설치 뒤 첫 데몬 시작 포함, gated 측정 아님. 단축키 → palette(A3)·stopped·S7-14 R 재측정은 NOT_RUN |
| SC3 실제 hook waiting/turn-end → 목록·Windows 알림 ≤2 s | NOT_RUN | 실제 hook 측정이 없다. S7-14 hook 행(H1–H4 턴 종료, P1)은 Q1·Q2·Q6로 NOT_RUN. H1X/H2X-TRUST PASS는 신뢰 화면 행(기준 7.0 s)이다. S1의 fixture·E2E 시간은 대신하지 않는다(07c §14) |
| SC4 창 닫기·데몬 재시작·업데이트 뒤 WSL 작업 유지 | NOT_RUN | S6-6b NOT_RUN(사유 `19cff21e-d2254acc` `acceptance.json` notRun). 그 뒤 SC4 측정 기록이 없다. 이전 source: S1 수용 "업데이트 뒤 세션 유지" PASS(`c28e4cab`) |
| SC5 생성된 오류 코드 전수의 문구 = 원인 + 다음 행동(internal은 진단 ID) | NOT_RUN | S6-6b 설치 수용 범위 밖이라 실행하지 않았다. 알려진 빈 곳은 §4(`[agent] copy`, Fix D §6 화면, RunTab "undefined") |
| SC6 UI 변경 → browser ≤2 s, 데몬 변경 → 재연결 ≤15 s | NOT_RUN | 계측만 있다(S6-4b `496b1c78`·`7d638599`). 실측은 S6-6b 범위 밖이었다 |
| SC7 PR CI ≤15분, 로컬 전체 ≤10분, release ≤40분, `.github`+`scripts` ≤2,000줄·workflow ≤3 | FAIL(미달) | PR CI의 Rust (Windows) job이 #648 27m24s·#649 17m47s·#651 첫 head `40d97a83` 34m57s로 15분을 넘었다. #651 최종 head `400a737f` 14m50s·main `842fa82e` 21m57s(job별 raw, 판정 아님). 로컬 전체·release ≤40분은 판정하지 않았다(S7-13 `pnpm check` 1,508.6 s, rc.2 release run `41604456654` job별 11s·13m30s·7m21s·14s). 1,982/2,000줄·workflow 2는 한도 안 |
| SC8 제품 코드 ≤20만 줄(`bash scripts/loc.sh 200000`, 관찰 트리거) | NOT_RUN | S6-6b 설치 수용 범위 밖이라 재지 않았다. 이전 기록: S2 #642 2회차 규모 52,258줄 |
| SC9 작업 없는 데몬 RSS ≤60 MB·CPU ≤0.5% | NOT_RUN | S6-6b: idle 데몬 표본이 harness 밖. 이전 source: S2 Task16 PASS(CPU 0.233331%, peak RSS 39,899,136 B, 61표본 60 s) |
| SC10 bridge → daemon → PTY → tmux echo p50 ≤10 ms·p95 ≤25 ms, 100 MB 출력 중 ping p95 ≤100 ms, 종료 뒤 입력 ≤1 s | NOT_RUN | S6-6b: terminal probe build를 옮기지 않았다. 이전 source: S1 설치본 수용 PASS(100 MB 출력 5.2 s, 1 byte 왕복 p95 2.5 ms, `c28e4cab`) |

그 밖에 공개 판단에 함께 낼 것(판정 그대로):
- S6 묶음(`19cff21e`): A6 FAIL(silent uninstall 직후 uninstall 등록 key를 확인해 실패, 실행 뒤에는 없어 비동기 제거로 판단), A3·A5 NOT_RUN. S7-14의 재실행은 interop 고장으로 NOT_RUN.
- S2–S5 설치 수용의 과거 결과(S2 1–11 NOT_RUN·12 FAIL·prod NOT_PROVEN, S3 4/6/7/8·autosave 1초, S4 실제 LSP 2/3/4/7, S5 5/6·외부 LAN·실제 도구 설치 NOT_RUN)는 S6-6b가 다시 실행하지 못해 그대로다. S6 최종 source 확인 중 HTTP → 노트·editor handoff·OAuth 승인·다중 worktree 복사 분리·dependent/sibling·updater all-window도 NOT_RUN이다(`19cff21e-d2254acc` `acceptance.json` notRun, 07c §14).
- RC1·rc.2의 Windows 설치·실행 수용은 NOT_RUN이고 RC → 1.0.0 수동 설치는 시험하지 않았다.
- release publisher의 stable 경로는 실제 GitHub에서 실행된 적이 없다(rc.1·rc.2는 prerelease 경로). 첫 실행은 S8-5다(§4 ci-docs M2).
- main은 S8 PR이 머지되기 전까지 `1.0.0-rc.2`라 updater가 꺼져 있다(RC M4, 버전은 S8-1 `a6f75a68`).
- **[1.0.0 전에 판단]** 고지 두 건(§4 문서·고지). 공개 버전 사이의 실제 업데이트는 다음 patch 때까지 NOT_RUN이다(07c).

## 4. 열린 후속

리뷰 상한 뒤 지적과 과제가 미룬 항목 가운데 고치지 않고 남긴 것이다. **[차단]**이 없으면 1.0.0을 막지 않는다. 출처·근거 경로·검토 판정은 이전 전문 §8([docs/history.md](../../../history.md))에 있고 ID로 찾는다. 닫으면 여기서 지우고 PR 본문에 근거를 적는다.

- **에이전트·하네스**
  - Codex 보내기(S7-5): tmux·pane 캡처 실패가 다음 행동 없는 internal 오류로 끝난다.
  - Codex 보내기(S7-5): Idle pane의 마지막 6줄이나 화면에 패턴과 같은 글(기록에 남은 `/ D I F F`, 선택지 줄)이 있으면 화면이 바뀔 때까지 보내기를 거절한다(닫힘 쪽, 낮은 확률).
  - 알아보지 못하는 Codex 화면(Fix A 3회차): 끊긴 동안의 초안, "Input disabled.", `/`·`@` 목록이 열린 초안, ChatGPT 로그인, Enter 힌트 없는 짧은 로딩, 힌트가 잘리는 좁은 터미널, 키 재지정. 받아들인 과일치: 전체 화면 기록의 활동 살펴보기.
  - 알려진 한계(Fix A 4회차): pi 셸 모드(테두리 색만 바뀐다, 제품 결정), 좁은 터미널의 Codex `Shell mode`·/side 라벨, Codex 외부 편집기(Ctrl+G).
  - 보이지 않는 제어 문자가 든 글은 `agents.control_characters`로 거절된다(F2, 초안은 남는다). 후보: `quoteForPrompt`가 control picture로 보이고 글자 이름을 말한다.
  - 정확히 `exit`·`quit`·`:q`만 보내면 opencode가 끝나는 것으로 추정된다(F3, `-s <id>`로 재개). 정확한 낱말의 MCP 거절은 나중에 더할 수 있다.
  - bracketed paste를 켜지 않은 도구 상태에서는 여러 줄 글이 줄마다 제출되고 `!`·`/`로 시작하는 뒷줄이 MCP 거절과 상관없이 셸·도구 명령으로 실행된다(Ruling 58, SECURITY.md에 적었다).
  - opencode(S7-7): cost 합산 없음 · subagent 토큰 빠짐 · 사용자 `OPENCODE_CONFIG_CONTENT`를 데몬 환경 값으로 대체 · idle로 버린 요청의 문구 미갱신 · 루트 idle 뒤 자식 요청 남음 · 빈 PATH 항목 처리 차이.
  - opencode Fix B: env 옵션 없이 띄우는 `Process.spawn` 호출자를 다 세지 못했다(Bun Worker의 `process.env` 차이). 에이전트가 닿는 spawn은 모두 spread라 수정은 유효하다(S7-14 H3).
  - pi(S7-8): 사용자 `sessionDir` 사용량 미집계 · Codex 시작 창 경합 · `~/.pi/agent` lock(pi 자체 동작) · "확장 없음" 배너 오표시 두 경우(ready 보고 유실, setup 90초 초과) · pane 패턴은 pi 1.3.0 문구 기준.
  - 재개: session id가 없는 Claude·Codex·opencode 작업은 원래 지시로 새로 시작한다(S1부터). 최종 검토에서 다시 본다.
  - 브랜치: 이름이 정확히 `agent`인 브랜치가 있으면 모든 `agent/…`가 막힌다(준비 단계 실패, 손실 없음) · 기본값과 같은 입력을 '고침'으로 보는 것을 고정하는 시험 없음 · 칸을 비웠을 때 설명 없음.
  - 준비: `[agent] copy` 실패가 다음 행동 없는 internal 오류로 끝난다(S7-11, SC5).
  - 환경(S7-14): 셸에만 있는 API 키는 에이전트 작업에 넘어가지 않는다(설계, README.md:44, `crates/cli/src/setup.rs` `ALLOWED`). 후속: 환경 점검이 셸에만 있는 `*_API_KEY` 이름을 알리고 각 CLI 로그인을 안내한다(값은 다루지 않는다).
  - 경로 찾기(S7-15 M-1): 조회 셸 `bash -lic`가 `~/.profile`·`/etc/profile.d`를 조회마다 실행해 로그인 파일이 띄우는 데몬(예: `ssh-agent`)이 조회마다 남을 수 있다(이 PC 미확인). 후속: README "도구 경로 찾기" 옆에 문서화한다(rc.2 릴리스 본문에는 적었다).
- **폐기·정리(O5, Fix D)**
  - **버리기는 작업 폴더 자신의 하위 모듈 사본(`.git/worktrees/<이름>/modules`)에만 있는, push하지 않은 커밋을 지운다.** Fix D 3차의 확인 창 경고는 막지 않는다. 후보: `has_submodules`로 버리기를 거절한다.
  - (기존) git이 숨기는 변경(assume-unchanged·skip-worktree)만 있는 작업 폴더는 그 변경째 지워진다(Fix D 2차).
  - 파일이 약 30만 개를 넘는 프로젝트(index 32 MiB 상한)는 작업 폴더 정리가 모두 내부 오류로 멈추고 목록에도 없다. 정리 명령이 없는 폴더에 진행 중인 Git 작업 표시(`MERGE_HEAD` 등)가 있으면 중단할 때까지 정리를 거절한다(Fix D 2차).
  - 버리기는 fingerprint 확인을 정리 명령 전에 하므로 정리 명령이 도는 동안 다른 프로세스가 만든 변경은 확인 없이 버려진다(Ruling 65 (5)).
  - 버리기 확인 창은 하위 모듈 검사가 답을 못 하면(index 32–64 MiB) "하위 모듈이 있습니다"를 보인다. 3상태 flag로 정확하게 할 수 있다(Fix D 3차).
  - P-OBS-1(S7-14): 버리기의 teardown이 실패하면 세션은 끝났는데 작업이 `running`으로 남는다(다시 버리면 `closed(discarded)`). 후속: 실패한 버리기가 열린 작업을 어떻게 보일지 정한다.
  - `discard_check`의 `git status` UTF-8·8 MiB 제한 · 시험 probe·Cap 레지스트리가 프로세스 전역 · 64 MiB 목록의 일시 메모리 · walk 사유가 `DiscardSnapshotUnavailable`로만 보임 · 무시된 하위 폴더 안 파일 추가·삭제 시험 없음(Fix B).
  - 정리 알림 제목 "끝나지 않은 Git 작업(병합·rebase·cherry-pick 등)"의 rebase는 거의 닿지 않는다(`cleanupMessages.ts`, Ruling 75, 1.0.0 전 아님).
- **Windows(S7-10, O1b, Fix C, S6-6b 검토)**
  - holder·bootstrap 신원 doc 주석 · `bootstrap_identity` probe 검증 단언 · JSON `null` holder 필드는 Legacy(도달 불가) · 늦은 저장 실패 알림에 이름 없음(O3).
  - Fix C: 자식이 정지를 넘겨 살아남고 reserve가 busy로 남는 service 수준 시험 없음 · 10초 poll과 `restart()` 봉인 사이의 드문 경쟁(닫힘 쪽) · "최신 시도"가 SQLite rowid 순서에 기댐 · holder가 준비 전(75)·`assigned` 전에 끝난 실행의 활동은 알 수 없음으로 남음 · `containers_malformed_and_output_bounds` 전체 실행 1회 실패(`pnpm check`에서 다시 보일 때만 기록).
  - 영역 3 M-2: holder가 이미 죽어도 bootstrap은 15초를 다 기다린다. M-5: 시도마다 만든 비공개 파일 다섯 개와 쓰다 죽은 `*.tmp-*`가 남는다.
  - S6-6b runtime I2 잔여: probe마다 PowerShell 기동과 `Add-Type` 컴파일(G2 실측 한 번 0.67–0.91초, 예산 안). reconcile 10초 polling의 누적 CPU와 native 경로 비용은 NOT_RUN.
  - S6-6b runtime M1·M2·M3·M4·M6(모두 fail-closed): `windows.prepare` 전 실패 시 reader가 닫히지 않음 · owner id가 환경 의존 · 큰 repo의 Paths probe 지연 · `ports_hint` 64개 초과는 늘 not ready · native 폴백의 관찰 deadline 20초.
  - S6-6b native M1–M8: Unknown attempt가 세션 내내 설치 차단 · `update_install_*`에 `main_only` 없음 · 30초 timer가 `Cancelled` · 남은 취소 stage를 `InstallFailed`로 분류 · `disarm` 무한 대기 · `Prepared::commit` 재해시가 UI thread·락 안 · 다운로드 300초 고정 · 소문자 "devbox" 창 제목.
  - `19cff21e` 검토 minor: 순서 assert가 문자열 일치뿐 · 사용자 명령이 exit 259로 끝나면 identityMismatch가 이어지는 모호함.
  - 수용 harness: A6은 uninstall 등록 key 제거를 bounded 대기한 뒤 판정 · A5는 vault 경로가 bundle 입력에 닿는 fixture와 검토된 UI helper 뒤 실행 · A3는 단축키 등록 증명 helper를 검토받은 뒤에만 실행.
  - 수용 보고: 사용자의 v0.6 `wsl-desktop.exe`가 `%LOCALAPPDATA%\Devbox\integration\**`에 계속 쓴다. v0.x 제거 전까지 "v1 prod unchanged; legacy v0.x external writer changed N file"로 보고한다.
- **앱·설정**
  - setup이 파일을 쓴 뒤 재시작 전에 끊기면 다음 자동 시작이 재시작을 생략한다([다시 설정]으로 복구).
  - `api.redaction_failed`(matcher)의 다음 행동이 약함 · 새 설치에서 [나중에]가 `autostart:true`를 기록(무해) · `redactKnownTokenPatterns`의 `sk_`·`sk-`에 왼쪽 경계 없음(데몬과 함께 고친다) · 노트가 꺼진 동안 search.notes 색인이 낡음 · `SupportBundle`의 `notes_disabled`에 다음 행동 없음.
  - 영역 5 M6: 1 MiB를 넘는 로그 tail은 자르지 않고 실패한다. M7: bridge 재연결 루프가 200 ms 바닥에서 돌 수 있다.
  - Fix E: [다시 시도]로 깨진 `settings.json`에서 회복해도 다시 열 때까지 전역 단축키 미등록·트레이 옛 오류(M2) · 자동 시작 설정 명령이 설정 오류 원문을 보임 · `refused`가 Welcome 뒤에도 남음(nit).
  - Fix D §6: 몇몇 화면이 오류 제목만 보인다(`pasteImage.ts`, `ReadOnlyBar`, `ActivityDay`, `ActivityWeek`, `FileOperationDialog`, `FileTree`, `SourcePage`, `QuickOpen`). `RunTab.tsx`는 다음 행동이 없는 코드에서 "undefined"를 찍는다(`messageText`로 고친다).
  - Fix F: `config.local.toml`의 나쁜 규칙은 설정 → 활동에서 저장해도 다음 시작까지만 기록이 재개되고 편집기는 그 파일을 말하지 않는다. 편집기는 합친 `[activity]`를 보이지만 다섯 키를 모두 `config.toml`에 쓴다.
  - Fix F(선택): 읽기 전용 미리보기도 tmux 서버 버전을 묻는다 · 규칙을 읽지 못하는 동안 Windows 수집기가 앱 이름 전송을 멈출 수 있다(데몬은 저장하지 않는다).
  - `projects.in_use` 안내의 "열린 작업" 모호함(`agentMessages.ts`, S6-6b front M3 잔여) · OAuth 승인 배너가 승인 호스트를 보이지 않음(front M4, `RequestEditor.tsx`). 실행기 패널의 OAuth 승인 연결(O7)은 1.0 이후다.
- **API·노트**
  - S6-6b api M3: 장시간 MCP-HTTP/SSE 세션이 Set-Cookie seed를 쌓아 약 1,000건 뒤 `api.redaction_failed`로 끝날 수 있다. M4: 잘못된 Basic credential의 디코딩된 비밀번호는 seed되지 않는다.
  - S6-6b docs M1–M6: 영수증 파일 미정리 · holder 거절 뒤 Attempted 잔류 · `notes.select_response_target`이 query인데 상태를 바꿈 · 취소·stale rename 캡처 무응답 · 캡처 BFS가 `target/`·`node_modules/`·`.git/`을 건너뛰지 않음 · 끝나지 않은 proof가 다음 rename을 30초 막음.
- **시험·크기**
  - 시험이 `/tmp/tmux-<uid>`에 socket을 남긴다 · tmux.conf와 `apply_extended_keys`의 옵션 중복, format 줄 시험 없음.
  - 거의 꽉 찬 파일(2026-11-02 `wc -l`): `execution.rs` 600/600, `TerminalView.test.tsx` 592, `cli/tests/agents.rs` 591, `config/lenient.rs` 590, `windows_observation.rs` 573, `terminal/src/tmux.rs` 572, `prepare.rs` 567, `discard_snapshot.rs` 556, `command_path.rs` 542. 다음 추가에는 나눈다.
  - T1b: ignored systemd wrapper 시험 4개(`runtime_secrets.rs` 등)는 owned-copy 시험 seam이 없어 실행해도 통과할 수 없다(NOT_RUN). ignore 사유도 실제 원인을 가린다.
  - CI E2E의 tmux 3.4 서버 비정상 종료(S1 `c68c923e`, 재실행 통과): 원인 미확정. 재발하면 원인을 고치고 재시도 래퍼는 쓰지 않는다.
- **문서·고지**
  - **[1.0.0 전에 판단]** 고지가 싣지 않은 코드: toolchain이 링크하는 musl·Rust 표준 라이브러리와 `webview2-com-sys@0.39.1`이 정적으로 링크하는 WebView2 로더(`WebView2LoaderStatic.lib`, 라이선스 파일 없음). 고정 절을 생성기에 더하거나 하지 않기로 한 결정을 적는다.
  - **[1.0.0 전에 판단]** 내장 `D2Coding.woff2`가 NAVER 공식 배포본인지 변환본인지 확인한 사람이 없다(OFL Reserved Font Name). 출처를 찾고, 변환본이면 이름을 바꾸거나 배포본에서 직접 만든 woff2로 바꾼다.
  - 고지에 라이선스 파일이 없는 패키지 21개(`@tauri-apps/plugin-opener`는 `LICENSE.spdx`만) · `COPYRIGHT`·`UNLICENSE`·`LICENSES/`는 이름 규칙 밖이라 읽지 않는다.
  - 고지 지문 밖의 입력 둘(생성기 코드 `xtask/src/notices*`, 글꼴 파일 자체): 바꾸면 `cargo run -p xtask -- notices`를 직접 돌린다.
  - README.md:153의 "성공 기준 SC2·SC6·SC10"은 사용자 문서에 남은 내부 ID다(Ruling 72, S8-2b).
- **성능·CI·릴리스**
  - SC2: 공개 전 반복 cold/warm 측정과 시작 경로 비용 분해 뒤 수정하거나 명시적 미달·다음 patch 계획을 정한다(S6-6b A2 raw만 있다).
  - SC7: Windows job 시간 분해와 ci-docs M4(musl release 시험이 모든 PR의 `linux-bin` → `Rust (Windows)` 임계 경로에 있다, 미계측). 필요하면 tag/dispatch에만 돌린다.
  - ci-docs M2: release publisher의 stable 경로는 첫 실제 실행이 S8-5다. scratch repo dry run은 실제 GitHub 대상이라 사용자 명시 승인이 필요하다. N2: x509(`-----BEGIN SIGNED MESSAGE-----`) 태그 서명이 릴리스 노트에서 지워지지 않는다.
  - #649 M2·M3: release 시험이 어느 검사가 거절하는지 고정하지 않음 · rc receipt-version 불일치 시험 없음.
  - Dependabot의 npm 보안 갱신 job이 반복해서 실패한다(seroval·katex·`@vitest/mocker`·undici, 원인 미조사, 1.0.0 뒤). S8 PR 머지 뒤 경고 7건이 닫혔는지 확인한다.
- **Windows·에이전트 실기(S7-14 NOT_RUN, 실행하지 않으면 PASS가 아니다)** — 정본은 [계획 S7-14 표](10-s7-s8-release.md#s7-14-windows-후보accept-수용), 실행 결과는 이전 전문 §5 S7-14 표다.
  - K1: tmux 3.4 호스트와 3.5 이상 호스트의 Shift+Enter(pi 줄바꿈 포함), 3.5 경계, WebView2, 호스트 tmux를 올린 뒤의 동작(K1X-TMUXUP은 Q3 미승인).
  - H1: Claude 폴더 신뢰 대화가 새 작업 폴더에서 뜨는지, 셸 모드·`?` 도움말 실화면. H2: Codex 재개, `codex resume` 직후 1초 안 보내기, `/feedback`·실화면 목록, 외부 편집기.
  - H3: 실제 opencode·pi, opencode 환경(`DEVBOX_AGENT_ID`·`DEVBOX_AGENT_LAUNCH` 없음)·중첩 `opencode run`·셸 모드 힌트·`exit`/`quit`/`:q`의 효과.
  - pi 행(H4, K1X-SEND, SN2의 pi 부분): Q6 뒤 pi가 쓸 수 있는 모델을 가지면 다시 돌린다. 시작점은 harness 검토 7 Note B(재시도 대기 중 idle 보고 여부).
  - P1: 실제 Claude 승인 상관과 Codex exec 승인. U1: rc.1 Claude 작업의 승인 뒤 보내기 막힘과 재개 회복.
  - W1 추가: 빌드 서버가 사본을 붙잡는 시간과 문구, `#[ignore]` bootstrap 시험, busy 없는 정지·수정·재시작, 콘솔 창 없음, cold 시작 35초, 느린 holder 뒤 업데이트 설치(W1 (다)는 Q4 미승인). readiness의 direct/native 동등성(S6-4a `313a6095`).
  - 연결·설정: STOP한 데몬에서 앱 안 회복, 강제·빠른 재시작의 MainPID, [나중에], [자세히], `settings.json` 깨짐, 느린 정상 시작의 [다시 시도]. O1a·O1b: Windows 실제 재시작·연결 유지. O6b: `mark_autostart_asked`.
  - 정리·버리기: 실제 정리 명령, 스쿼시로 닫은 작업의 실패한 정리, 하위 모듈 작업 폴더(CL3 부분, CL4–CL6).
  - Windows 절반: CS3·C1·M4(WSL 절반은 통과), N1 accept 설치 폴더의 고지 파일, S6 묶음 G1·G2·A1·A2·A4·A6 재실행.
  - S7-11 문서의 확인 대기 자리: 서명 없는 설치 파일의 Windows 경고(SmartScreen), 앱 제거 뒤 남는 항목(NSIS 제조사 key 포함, 대화형 제거 미확인), MCP 등록 명령의 설치본 재확인.
- **운영**
  - 2026-10-22 WSL 재시작 뒤 `user@1000.service` 실패로 메모리 cgroup이 걸리지 않았다(`memory_enforced:false`). 그 뒤 검증은 자식 환경에만 `XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`를 줘 `memory_enforced=true`를 확인하고 돈다(호스트 서비스는 바꾸지 않는다). 걸리지 않으면 실행 결과에 미적용을 적는다.

## 5. 유효한 결정

- 사용자 결정 1–6(2026-10-27): [계획 10 §1](10-s7-s8-release.md#1-사용자-결정-2026-10-27)
- PR 리뷰(결정 5)·릴리스(main에서, 2026-10-26)·브랜치와 커밋: [CONVENTIONS §8](../../../../CONVENTIONS.md#8-git)
- 검증·자원 제한·target 정리(결정 6)·Windows 수용(2026-10-13·14): [CONVENTIONS §5](../../../../CONVENTIONS.md#5-검증)
- 인계·먼저 물을 것·과제 검토 기준: [CONVENTIONS §11](../../../../CONVENTIONS.md#11-기록과-인계) · [AGENTS](../../../../AGENTS.md)
- 설계 결정 D1–D16과 설계 질문 Q1–Q4: [01-design §0](01-design.md#0-결정-요약) · 성공 기준 SC1–SC10: [01-design §1](01-design.md#1-목표와-성공-기준)
- 2026-10-08–11-03 결정 표의 원문(rc.2 공개와 S8 의존성 포함 승인 포함): 이전 전문 §2([docs/history.md](../../../history.md))
- ADR: [목록](../../../adr/README.md) · [0016 개인용 보안 범위](../../../adr/0016-personal-security-scope.md) · [0017 v1 구조](../../../adr/0017-rebuild-single-app-wsl-daemon.md)

## 6. 근거 보관

기록이 가리키는 근거는 **이 PC에만 있다**. 저장소와 GitHub에는 없고 Git common-dir `/home/jihoon/projects/devbox/.git` 아래에 둔다.

- `devbox-verification/`: 실행 근거. S7·S8은 `s7/`(`harness/`, `runs/<실행>/`, `release-v1.0.0-rc.2/`, `windows-run-rc2*/`, `tmp-archive-2026-11-03/`).
- `devbox-archive/`의 압축 보관. 기록의 `devbox-verification/...` 경로는 보관 안의 같은 상대 경로다.
  - `local-verification-20261021.tar.gz`와 `local-verification-20261021.manifest.json`(2026-10-21 정리).
  - `local-verification-20261025.tar.gz`(S3–S5, SHA-256 `cb08876abd0a863c653263369a03eeb65d0548673999cae2b8c72cc03bfc1dbf`)와 manifest. 정리 근거는 압축 밖 `cleanup-20261025/`.
  - `cleanup-20261026/local-verification-20261026.tar.gz`(S6·RC, SHA-256 `a292415367811796f7f16efcf5ce2b02eb69be4e2e76b08ca0d1e67ea8afecea`)와 manifest·`deletions.json`·`archive.py`. S6 worktree 원장은 `wt-s6-sdd/sdd/`, RC는 `wt-rc1-sdd/sdd/`, 임시 파일은 `tmp-s4acc/`·`tmp-s6post/`.
- 과제 작업 공간(SDD)과 Ruling 근거: 이 worktree의 `.superpowers/sdd/10-s7-s8-release/`(git 무시). worktree를 지우기 전에 common-dir로 옮긴다(S7-15 선례).
- 다음 세션은 manifest와 보관 파일을 확인하고 필요한 근거만 읽는다.

## 7. 최근 세션

최근 10줄만 둔다(§0). 그 전 줄은 이전 전문 §6([docs/history.md](../../../history.md))에 있다. 줄 안의 § 번호는 그 줄을 쓸 때의 구성이다.

| 날짜 | 하네스 | 한 일 | 남긴 상태(브랜치·커밋·다음 단계) |
|---|---|---|---|
| 2026-10-21 | Codex 하네스 구현·IndexDb/CodeView 분담 | Task15b 공용 색인 DB·코드 보기·검사 대기/독립 잠금·tmux 진단, 고유73개·타입/생성·직접 영향 검사 PASS | 독립 최종 검토 PASS·미해결0·Biome344 PASS, 로컬 완료 커밋. 제품29/문서2/artifact77 해시 일치, scripts1497줄/2 workflows, 다음 Task15c. 전체 검사·Windows·push/PR Task16. 근거 `devbox-verification/s2-task15b/{summary,source-freeze,test-names,artifact-hashes}.json` |
| 2026-10-21 | Codex 하네스 구현·WSL/패키징 분담 | Task15c 단일 채널 표·Windows/WSL 설치 격리·receipt 검증 구현, 고유79개·MSVC 좁은 compile·PowerShell 모의2 suite PASS | 독립 검토 PASS·미해결0·Biome344 PASS, 로컬 완료 커밋. 제품30/문서2/artifact81 해시 일치, scripts1500줄/2 workflows, 다음 Task16. 실제 Windows 설치·전체 검사·push/PR 미실행. 근거 `devbox-verification/s2-task15c/{summary,source-freeze,test-names,parent-commands,artifact-hashes}.json` |
| 2026-10-21 | Codex 하네스 관찰 UI 구현 | Task16 승인 설계 누락 보완: 프로세스 우선 목록·종료 확인, 포트 브라우저·소유자 종료. RED Rust1/UI3→직접 고유33개(프런트21·Rust12) PASS | `PortInfo.startTimeMs`는 fd 스캔 전후 시작 tick 일치에서 생성, 미확인/container는 종료 불가. 확인 snapshot 고정·stale 오류 후 갱신 완료까지 잠금/새 확인 회귀 PASS. 생성/타입·독립 검토·완료 Biome350 PASS, 미해결0. 제품21/문서2/artifact54 해시 일치, Biome0.896초·cgroup122,421,248B·swap0. 완료 커밋·문서 해시는 root-completion.json. 전체 검사·SC9·Windows·CI·PR 미실행. 근거 `devbox-verification/s2-task16/observe-ui/{summary,source-freeze,test-names,commands}.json` |
| 2026-10-21 | Codex 하네스 통합 수정·별도 브랜치 검토 | Task16 첫 전체 검사 실패 보존→직접140개·strict Clippy·E2E3 수정 PASS, SC9 CPU0.233331%/RSS39,899,136B PASS | 제품75개 동결·독립 검토 PASS·미해결0, 규모52,174줄·scripts1,500줄. 완료 Biome/커밋 뒤 최초 push·동일SHA Windows accept 수용·단일PR. 현재 설치 수용·CI·리뷰 미실행, S2 수용 완료 아님. 근거 `devbox-verification/s2-task16/` |
| 2026-10-21 | Codex 하네스 PR #642 1회차 수정 | Windows CI69개·Prod/Accept 패키징 PASS, 설치 전 errno110 FAIL·실기 미실행. 코드 리뷰 scheduler1건·CI 세션 assertion1건 보완 | 직접26개·영향 strict Clippy·별도 검토 PASS. 실제 CI 오류 UNKNOWN·원본 실패 보존. 첫 보안 리뷰 지적0건 완료·Biome 뒤 묶음 push, 최종 필수CI/후속 리뷰/머지 대기. 근거 `devbox-verification/s2-task16/pr/` |
| 2026-10-21 | Codex 하네스 PR #642 2회차 수정 | 코드3건·보안0건 판정, readiness 귀속/선택 프로젝트/비밀키 DB 실패 복구 및 CI 정리·터미널 준비·로그 flush 보완 | 직접 고유95개·영향 strict Clippy·독립 검토 PASS, 실패 원본 보존. 완료 Biome 뒤 묶음 push, 최종 CI·3회차 리뷰·설치 시도·squash 대기. 근거 `devbox-verification/s2-task16/pr/round2/` |
| 2026-10-21 | Codex 하네스 PR #642 3회차 수정 | b24d99d5 Linux/Frontend/E2E CI PASS, 코드1건·보안0건 완료. 다른 등록 프로젝트 cwd의 Run 귀속 우선순위 보정 | RED→직접2개·독립 검토·영향 strict Clippy PASS. 자동 리뷰3회 상한 도달, 완료 Biome 뒤 최종 SHA CI·설치 시도·squash 진행. 4회차는 차단 기준만 적용 |
| 2026-10-21 | Codex 하네스 S2 머지·인계 | #642 → squash1e82e649, 최종f703 CI3/E2E PASS·리뷰3회 처리. 최종 설치 사전 검사 errno110 FAIL·실기 미실행 보존 | 검증tree동일. 사용자 후속 요청으로 결과 문서 PR·근거 압축 보관·로컬 정리 진행, 다음 세션은 깨끗한 main에서 시작하도록 인계 변경. 상한 이후 Windows 준비 지적1건 §8 이월, S3/S5 제품 미착수. 정리 완료 여부는 실제 Git/PR·manifest로 확인 |
| 2026-11-02 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S7-13 마무리(영역 6 재검토 Ready, 버전 `1.0.0-rc.2` `22731033`, `pnpm check` PASS 1,508.6 s)와 S7-14: Windows 후보 `22731033`(run `41374497823` FAIL) → `240430cc`(run `41377466587`) → P-DEF-1 수정 `40d97a83`(run `41423905881` SUCCESS), harness 검토 6건·수정 4회차, WSL 수용 실행 4개(최종 `40d97a83-a4e9f78c` PASS 7행·FAIL 0, 업그레이드 `40d97a83-f6fd79ec`)와 NOT_RUN 이유(Ruling 90). 23:08에 세션이 다시 떴으나 WSL은 재시작되지 않았고(interop 여전히 `accept4 failed 110`) 사용자 지시로 WSL 재시작 요청(Q0)을 철회하고 남은 행을 NOT_RUN으로 두고 진행하기로 했다. 결과 기록은 patch(`s7-14-docs.patch`)로 만들어 S8-0으로 인계했다(Ruling 91). 23:55 KST에 PR #651을 열었다 | `v1/s7-rc2` `40d97a83` push, PR #651 열림(자동 리뷰 꺼짐 판정 창 10분). 다음: 독립 검토 1회·CI·머지. 근거 Git common-dir `devbox-verification/s7/` |
| 2026-11-03 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S7-15: 독립 PR 검토(CHANGES REQUIRED, Critical 0·Important 2·Minor 1)와 수정 두 건(`4c95c932` tmux 3.4 시험, `400a737f` I-2 `[agents]` 실패 정책, Ruling 93·95), 최종 head `400a737f`의 CI·수동 Windows run `41536487871`·재확인 실행 `400a737f-11d1bb99`(binding 검토 7 REFUSE → 31 case APPROVE, Ruling 96), PR #651 squash 머지(12:45 KST → `842fa82e`), tree 대조·main CI·S7 worktree/브랜치/`/tmp` 정리. S7-16: 사용자 승인 뒤 `v1.0.0-rc.2` 태그·prerelease 공개(17:35–17:57 KST, release run `41604456654`, asset SHA-256 세 곳 일치, `releases/latest` `v0.9.0`). 머지 뒤 Dependabot 경고 7건 → 사용자 승인으로 S8에 포함. S8 착수(Ruling 97: 사용자 확인 결과를 기다리지 않는다): worktree `v1/s8-release`, S8-1 `a6f75a68`(버전 `1.0.0`)·S8-1b `eb08e39b`(zip 9.0.5·seroval 1.7.7)·S8-0 `cfa89f0a`(S7 결과 기록) | `v1/s8-release` `eb08e39b` 위에 S8-0 문서 커밋, push·PR 전. 다음: S8-2 문서 통폐합 → S8-3 검사·후보 → S8-4 PR → S8-5 공개(별도 승인). 근거 Git common-dir `devbox-verification/s7/`, 작업 원장은 이 PC의 `.superpowers/sdd/10-s7-s8-release/` |
