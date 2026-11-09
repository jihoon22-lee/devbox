# 진행 원장

devbox의 진행 상태를 적는 **유일한 원장**이다. 계획 문서의 체크박스와 상태 줄은 보조다. 세션을 시작하고 끝낼 때 [CONVENTIONS §11](../../../../CONVENTIONS.md#11-기록과-인계)대로 이 파일을 읽고 고친다.

## 0. 읽는 법

- 이 파일은 지금 상태만 적고 **200줄을 넘기지 않는다**(사용자 결정 4, S8-2). 줄이기 전 전문(S0–S8-0의 결정 표·과제 기록·계획 대비 편차·세션 기록)과 끝난 계획은 [docs/history.md](../../../history.md)의 고정 링크로 본다. 지난 결과·근거 경로·Ruling은 거기서 찾는다.
- 구성: 0 읽는 법 · 1 현재 위치 · 2 열린 결정 · 3 1.0.0 공개 게이트 · 4 열린 후속 · 5 유효한 결정 · 6 근거 보관 · 7 최근 세션.
- 이 파일은 PR과 함께 main에 들어간다. 작업 중인 브랜치가 있으면 **그 브랜치의 이 파일이 더 최신**이다(CONVENTIONS §11 세션 시작 2).
- 판정은 PASS·FAIL·NOT_RUN이다. 실행하지 않은 확인은 PASS로 쓰지 않는다. 결과는 그 결과를 낸 source SHA의 것이다.
- 날짜는 `YYYY-MM-DD`, 시각은 KST다. 근거 경로 `devbox-verification/…`은 이 PC에만 있다(§6).
- **세션 규칙:** 세션이 끝날 때 §7에 한 줄을 더하고 최근 10줄만 남긴다. 밀려난 줄은 이 파일의 git 이력에 남는다. 이 규칙이 "오래된 줄은 지우지 않는다"를 대신한다(근거: 결정 4).

## 1. 현재 위치

- **2026-11-09 · S8(1.0.0) 공개 전, rc.3 검토 수정 중. S7·S8-4·S8-4b는 끝났다:** S7은 PR #651 → main `842fa82e`(2026-11-03)와 `v1.0.0-rc.2` prerelease(release run `41604456654`), S8-4(버전 `1.0.0`·보안 갱신·고지·문서 통폐합)는 PR #653 → `08dffcd3`(2026-11-05), S8-4b(최종 교차 검토 수정, 계획 10 [§S8-4b](10-s7-s8-release.md#s8-4b-최종-교차-검토-수정--사용자-결정-2026-11-06))는 PR #654 → `70b1b4f6`(2026-11-06 17:33 KST, tree = 검증한 head `db8e51f7`)이다. 검사·검토·CI 기록은 각 PR 본문과 SDD 원장(§6)에 있고, 줄이기 전 §1은 `70b1b4f6`의 이 파일에 있다. main은 S8-5까지 `70b1b4f6`에 고정했고(Ruling 144) 이 브랜치의 squash 머지로 그 고정이 끝난다. `releases/latest`는 `v0.9.0`이고 공식 `v1.0.0` 태그·Release는 없다.
- **이 브랜치:** `fix/v1-rc3`, worktree `/home/jihoon/projects/devbox-wt/fix-v1-rc3`. 2026-11-08 origin/main `70b1b4f6`에서 만들었고 계획 10 [§S8-5b](10-s7-s8-release.md#s8-5b-100-rc3-검토-수정--사용자-결정-2026-11-08)를 따른다. 사용자 결정(2026-11-08)으로 main `70b1b4f6`을 검토자 5명이 세 단계(결함, 사용성·기능 연계, 디자인)로 검토하고 실제 claude·codex로 직접 시험했다(§3 2, 근거 `rc3-review/`). 그 결과를 범위 "결함+디자인+표시 개선 (권장)"·새 기능 넷·H-02 "고친다 (권장)"로 고치고 `v1.0.0-rc.3` prerelease까지 낸다. 정식 1.0.0은 계속 보류다(§2). 과제 커밋은 S8-5b-0 `9d8640af`(계획)부터 화면 확인(Ruling 165, 20항목) 뒤 수정 `3160aeca`까지 48개와 S8-5b-25 문서(이 커밋)이고, 과제마다 구현 → 별도 검토 → 필요하면 수정·범위 재검토를 거쳤다. push·PR 전이다. 이 PR의 Windows accept 설치 스모크는 NOT_RUN이고 S8-5에서 최종 후보로 한다(Ruling 160). **다음:** S8-5b-26 버전 `1.0.0-rc.3` → 27 브랜치 전체 검토·`pnpm check` → 28 push·`windows.yml`·PR·§8 리뷰·squash → 28b 머지 뒤 main SHA로 H-01·H-02 실제 확인(Ruling 163) → 29 rc.3 태그·prerelease → 30 정리. 그 뒤 S8-5(버전을 `1.0.0`으로 되돌리는 PR, 그 SHA의 Windows 확인, 공개 승인은 따로 묻는다).

## 2. 열린 결정

사용자만 정할 수 있는 것이다. 답이 오면 여기서 지우고 §3·§4에 반영한다. 승인 원문과 닫힌 결정은 이전 전문 §2([docs/history.md](../../../history.md))에 있다.

- **① WSL→Windows interop(W1):** 2026-11-06까지(마지막 확인 17:36 KST) `cmd.exe /c ver`가 `UtilAcceptVsock:273: accept4 failed 110`으로 실패했다(VM 전체, 2026-10-21 S2 설치 수용 때와 같은 증상). 멈춘 WSL `Relay` 프로세스가 VMBus relid를 다 쓴 탓이었고, 2026-11-07 사용자가 그 프로세스를 sudo로 끝내 WSL 재시작 없이 돌아왔다(`devbox-verification/s8/interop-diagnosis-2026-11-07-recovery.md`). 메모리 조각화로 다시 끊길 수 있어 Windows 배치마다 VMBus 채널 수와 멈춘 `Relay`를 확인한다. 사용자 지시(2026-11-02): WSL 재시작을 요청하지 않는다(Q0 철회). `70b1b4f6`에서 한 Windows 확인은 §3 1·I1에 있다. W1 (다)(빌드 서버) 자료가 나오면 Fix C의 "사용자 확인 뒤 남은 프로세스 끝내기"를 사용자가 정한다(Ruling 47).
- **③ S7-14 Q6:** opencode는 "This API key has expired…", pi는 모델 "Muse Spark 1.3 Free"의 "Model is unavailable"로 멈췄다(도구 자신의 인증·모델 오류). 에이전트 작업은 셸의 API 키 변수를 받지 않는다([README "에이전트 도구"](../../../../README.md#에이전트-도구)). 사용자가 본인 터미널에서 같은 프롬프트로 재현하는지 확인하기 전에는 원인을 단정하지 않는다(Ruling 87 보정).
- **S5 수용 3·6(2026-10-21, S5-9 수용에만 걸림):** 외부 서비스의 웹훅 수신(LAN 노출·포트 전달·방화벽)과 빠진 도구의 실제 설치(`sudo apt`·`npm -g`·`winget`). 기본안만 확인하고 실제 실행은 사용자 승인 뒤에만 한다(07b).
- **S8-5 `v1.0.0` 공개 — 보류(사용자 결정 2026-11-05 "보류: Windows 확인 뒤"):** S8-5 0의 버전 되돌림 PR squash SHA로 계획 10 S8-5 0의 확인(accept 설치 스모크·I1 W1·S8-4b-10 업데이트 확인·§3의 S8-5b Windows 행 등)을 먼저 하고, 그 결과로 공개를 다시 묻는다. 태그 push와 Release는 그 시점의 별도 명시 승인 뒤에만 한다. 2026-11-03의 rc.2 공개 승인과 S8 의존성 포함 승인, 2026-11-06의 수정 범위·계획 승인, 2026-11-08의 rc.3 검토·prerelease 지시(Ruling 149)는 공식 `v1.0.0` 승인이 아니다.

## 3. 1.0.0 공개 게이트

S8-5에서 사용자에게 제시한다(계획 10 S8-5). 기록된 판정만 옮기고 올리지 않는다. 사용자는 목록을 모두 직접 실행하지 않고도 승인할 수 있고, 사용자 몫은 결과가 없으면 NOT_RUN이다([07c S6-6c](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md#s6-6c-공개-직전-단-한-번의-명시-승인)). 실측이 미달이면 원인·최소 수정/재측정 또는 명시적 미달·다음 patch 계획을 적고, 근거 없는 목표 완화·측정 생략은 하지 않는다([07c §14 SC 원장](https://github.com/jihoon22-lee/devbox/blob/842fa82e403231e67fc4dfb7e0d64b2ecc2af1d2/docs/superpowers/plans/2026-10-08-devbox-v1/07c-s6-release.md#sc별-판정-원장)). `40d97a83-…`·`400a737f-…`·`19cff21e-…`·`70b1b4f6-…`는 수용 harness 실행 ID다(§6).

| 항목 | 판정 | 근거 또는 이유 |
|---|---|---|
| 1 새 설치 첫 화면 | NOT_RUN | 최종 후보는 S8-5 0의 버전 되돌림 squash SHA다(그 전 후보 `70b1b4f6`·`08dffcd3`, 계획 10 S8-5 0). 그 SHA의 accept 설치 스모크(첫 화면·버전 표시 1.0.0)는 S8-5에서 한다(rc.3 PR은 하지 않는다, Ruling 160). 이전 source: `70b1b4f6` A1(설치)·A2(실행, 첫 데이터 6.5 s) PASS(`70b1b4f6-e228bf7f`, 재실행 `70b1b4f6-f5d3dcf8`)와 첫 화면·버전 표시 1.0.0 PASS(사용자 확인, 2026-11-07), S6-6b A2 PASS(launch, `19cff21e-d2254acc`). S7-14·S8-3에서는 interop 고장으로 하지 못했다 |
| 2 agent 전 과정 | FAIL | `70b1b4f6`의 결과다. 2026-11-08 WSL 직접 시험(S7-14 harness `wsl-fresh`, 실제 claude 2.1.318·codex 0.167.0, 실행 `70b1b4f6-58f1b658`·`70b1b4f6-8ad0ac09`·`70b1b4f6-a1667181`). 사용자가 2026-11-08에 폴더 신뢰 수락(Q1·Q2)을 승인해 두 도구의 신뢰 화면을 수락했다(Ruling 149, S8-5b-28b 재실행도 같은 범위다, Ruling 162). 그 항목은 `~/.claude.json`·`~/.codex/config.toml`에 남고 정리가 지우지 않는다. H1(claude)·H2(codex) 모두 (a) 실행 중 → 턴 종료 2초 안과 (d) 점검 행은 PASS, (b) 재개 뒤 새 지시 전까지 '실행 중'(H-02)과 (c) 열린 작업의 목록 행에 토큰 합계 없음(H-01)은 FAIL이다. H1X-TRUST·H2X-TRUST·H2X-RESUME·H2X-FEEDBACK(a·b)·CL1·CL2는 PASS다. H-02는 S8-5b-5, H-01은 S8-5b-5b가 고쳤고, 이 PR 머지 뒤 main SHA로 H1X-TRUST·H1·H2X-TRUST·H2를 다시 돌려 확인한다(S8-5b-28b, Ruling 163). claude 셸 모드 (a)(d)·도움말·승인 행, codex 화면 행 일부(H2X-SCREENS)와 opencode·pi(§2 ③)는 NOT_RUN이다(§4) |
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
| I1 Windows 사본 reader가 고아 자손을 놓침 **[차단]** | NOT_RUN | 결함은 S6-6b G1(`19cff21e`)이 실제 Windows에서 CONFIRMED했다. S7-10이 고쳤다(`681a7b82`·`64d14394`·`68bb9e82`, S7-13 Fix C 보강). 최종 후보의 W1은 아직 하지 않았다(S8-5 0). 이전 source: `70b1b4f6` W1 (라) PASS(실제 Windows의 `#[ignore]` 시험, 2026-11-07)와 (가)(나) PASS(`70b1b4f6-f5d3dcf8`, 2026-11-08: 고아가 도는 동안 reader가 열려 있었고 holder 영수증 뒤에 닫혔다). S7-14에서는 interop 고장으로 하지 못했다. 결정 2: 확인하지 못하면 1.0.0 공개를 막는다. 공개 승인 때 미검증으로 알린다(Ruling 90). rc.2는 이 상태로 공개했고 rc.3도 같다(Ruling 160, 계획 10 S8-5 0). 남은 위험: 같은 로그온 세션에서 holder만 강제 종료되고 bootstrap도 끝났는데 고아가 돌면 reader가 일찍 닫힌다 |
| S8-4b-10 업데이트 중 WSL 유지 (Windows) | NOT_RUN | accept 채널에서 tmux 작업이 도는 동안 fixture 업데이트를 실행해, 배포판이 멈추지 않는지(tmux 세션 유지)와 연결 공백 시간을 잰다(계획 10 S8-4b-10, 코드 `70310423`). `70b1b4f6`에서는 harness에 fixture 업데이트 실행이 없어 하지 않았다(2026-11-07). S8-5 0의 SHA에서도 못 하면 공개 질문에 NOT_RUN으로 낸다 |
| S8-5b 빠른 캡처·종료·화면 (Windows) | NOT_RUN | S8-5b의 Windows 쪽은 실제 Windows에서 돌리지 않았다(Linux 단위 시험, 브라우저 개발 모드 화면 확인, MSVC 대상 형 검사만 했다). S8-5b-22: native 명령 `capture_draft`·`capture_has_draft`·`capture_open`의 실제 호출, 종료 확인 창의 [캡처 열기]로 빠른 캡처 창 열기, 트레이 [종료]의 종료 확인 흐름(S8-5b-23 포함), 빠른 캡처 초안이 있을 때 실제 업데이트 설치 거절(`CommitFence`), 빠른 캡처 창을 다시 읽어도 남는 sessionStorage 초안, 잇단 초안 보고의 순서, 캡처 창이 없어질 때 초안 표시 지우기, WebView2의 클릭 `detail` 값(두 번 누르기·터치·키). 화면 확인 뒤 수정: 좁은 상태 표시줄의 native 업데이트 칸, 실제 Windows 고대비 테마(Chromium forced colors 흉내만 했다), UI 배율 popper 수정의 WebView2 표시와 실제 모니터 배율. 외부 편집기(VS Code·Cursor)로 여는 동작(S8-5b-15·18)도 열어 보지 않았다. S8-5에서 최종 후보로 한다 |
| SC1 흔적 없는 전용 Windows 계정에서 setup → 첫 데이터, 사용자 행동은 배포판 확인 1번 이하 | NOT_RUN | 전용 계정·독립 환경이 없다(S6-6b `19cff21e-d2254acc` `acceptance.json` notRun). 기존 사용자 자료를 지워 흉내 내지 않는다 |
| SC2 단축키 → palette ≤300 ms, cold → 첫 데이터 WSL running ≤3 s·stopped ≤10 s | FAIL(미달) | cold → 첫 데이터 raw 9036 ms(`19cff21e-85014e8e`)·6093 ms(`19cff21e-d2254acc`)·6.5 s(`70b1b4f6-e228bf7f` A2): 각 1표본, WSL running, 설치 뒤 첫 데몬 시작 포함, gated 측정 아님. 단축키 → palette(A3)·stopped·S7-14 R 재측정은 NOT_RUN |
| SC3 제목+도구+시작 1번으로 작업 준비·실행, 실제 hook waiting/turn-end → 목록·Windows 알림 ≤2 s | NOT_RUN | Windows 알림 ≤2 s와 실제 hook waiting(P1 승인 화면)은 재지 않았다. 실제 hook 턴 종료 → `agents.list` idle 2초 안은 `70b1b4f6`의 2026-11-08 WSL 직접 시험에서 H1(claude)·H2(codex) (a) PASS다(§3 2, 데몬 목록 기준이고 앱 화면·Windows 알림은 아니다). H3·H4(opencode·pi)는 §2 ③으로 NOT_RUN이다. H1X/H2X-TRUST PASS는 신뢰 화면 행(기준 7.0 s)이다. S1의 fixture·E2E 시간은 대신하지 않는다(07c §14) |
| SC4 창 닫기·데몬 재시작·업데이트 뒤 WSL 작업 유지 | NOT_RUN | S6-6b NOT_RUN(사유 `19cff21e-d2254acc` `acceptance.json` notRun). 그 뒤 SC4 측정 기록이 없다. 이전 source: S1 수용 "업데이트 뒤 세션 유지" PASS(`c28e4cab`) |
| SC5 생성된 오류 코드 전수의 문구 = 원인 + 다음 행동(internal은 진단 ID) | NOT_RUN | S6-6b 설치 수용 범위 밖이라 실행하지 않았다. 알려진 빈 곳은 §4(`[agent] copy`, Fix D §6 화면, RunTab "undefined") |
| SC6 UI 변경 → browser ≤2 s, 데몬 변경 → 재연결 ≤15 s | NOT_RUN | 계측만 있다(S6-4b `496b1c78`·`7d638599`). 실측은 S6-6b 범위 밖이었다 |
| SC7 PR CI ≤15분, 로컬 전체 ≤10분, release ≤40분, `.github`+`scripts` ≤2,000줄·workflow ≤3 | FAIL(미달) | PR CI의 Rust (Windows) job이 #648 27m24s·#649 17m47s·#651 첫 head `40d97a83` 34m57s로 15분을 넘었다. #651 최종 head `400a737f` 14m50s·main `842fa82e` 21m57s(job별 raw, 판정 아님). Rust (Windows)는 linux-bin 뒤에 시작하므로(최종 head linux-bin 11m05s) 최종 head의 windows run은 약 26분이다. 로컬 전체·release ≤40분은 판정하지 않았다(`pnpm check`: S7-13 1,508.6 s는 10분의 2.5배, S8-3 `70cd633f` PASS 1,267.4 s는 2.1배, 둘 다 10분 초과이고 판정 아님. rc.2 release run `41604456654` job별 11s·13m30s·7m21s·14s). 1,982/2,000줄·workflow 2는 한도 안 |
| SC8 제품 코드 ≤20만 줄(`bash scripts/loc.sh 200000`, 관찰 트리거) | NOT_RUN | S6-6b 설치 수용 범위 밖이라 재지 않았다. 이전 기록: S2 #642 2회차 규모 52,258줄 |
| SC9 작업 없는 데몬 RSS ≤60 MB·CPU ≤0.5% | NOT_RUN | S6-6b: idle 데몬 표본이 harness 밖. 이전 source: S2 Task16 PASS(CPU 0.233331%, peak RSS 39,899,136 B, 61표본 60 s) |
| SC10 bridge → daemon → PTY → tmux echo p50 ≤10 ms·p95 ≤25 ms, 100 MB 출력 중 ping p95 ≤100 ms, 종료 뒤 입력 ≤1 s | NOT_RUN | S6-6b: terminal probe build를 옮기지 않았다. 이전 source: S1 설치본 수용 PASS(100 MB 출력 5.2 s, 1 byte 왕복 p95 2.5 ms, `c28e4cab`) |

그 밖에 공개 판단에 함께 낼 것(판정 그대로):
- S6 묶음(`19cff21e`): A6 FAIL(silent uninstall 직후 uninstall 등록 key를 확인해 실패, 실행 뒤에는 없어 비동기 제거로 판단), A3·A5 NOT_RUN. S7-14의 재실행은 interop 고장으로 NOT_RUN. `70b1b4f6` 재실행(2026-11-07·08, `70b1b4f6-e228bf7f`·`70b1b4f6-f5d3dcf8`): A1·A2·A4·A6·M4·C1·N1-ACCEPT·W1 (마)(바)(사) PASS, G1·G2·W1-H(아)·CS3는 harness 결함으로 NOT_RUN(harness), W1 (다)는 Q4 아니오(Windows에 빌드 도구를 설치하지 않음)로 NOT_RUN.
- S2–S5 설치 수용의 과거 결과(S2 1–11 NOT_RUN·12 FAIL·prod NOT_PROVEN, S3 4/6/7/8·autosave 1초, S4 실제 LSP 2/3/4/7, S5 5/6·외부 LAN·실제 도구 설치 NOT_RUN)는 S6-6b가 다시 실행하지 못해 그대로다. S6 최종 source 확인 중 HTTP → 노트·editor handoff·OAuth 승인·다중 worktree 복사 분리·dependent/sibling·updater all-window도 NOT_RUN이다(`19cff21e-d2254acc` `acceptance.json` notRun, 07c §14).
- RC1·rc.2의 Windows 설치·실행 수용은 NOT_RUN이고 RC → 1.0.0 수동 설치는 시험하지 않았다.
- release publisher의 stable 경로는 실제 GitHub에서 실행된 적이 없다(rc.1·rc.2는 prerelease 경로). 첫 실행은 S8-5다(§4 ci-docs M2). main은 S8 PR #653(`08dffcd3`)부터 `1.0.0`이라 prod 정식 버전의 앱 업데이트가 켜진다(RC M4, 버전은 S8-1 `a6f75a68`). 공개 버전 사이의 실제 업데이트는 다음 patch 때까지 NOT_RUN이다(07c).
- 고지의 [1.0.0 전에 판단] 두 건은 닫혔다. 글꼴: `app/src/assets/fonts/D2Coding.woff2`(git blob `2f38577f23892226f450ead99fca0432f881e11a`, 1,490,832 B)는 저작권자 저장소 `naver/d2codingfont`의 커밋 `b0dc372e`("Release 1.4.0", `fonts.css`가 가리키는 커밋)에 든 `site/fonts/D2Coding-Regular.woff2`와 바이트가 같고, `LICENSE-D2Coding.txt`(blob `50a6c23e…`)는 그 커밋의 `OFL.txt`와 같다(`gh api repos/naver/d2codingfont/git/trees/b0dc372e…?recursive=1`, 2026-11-04). 저작권자의 자체 배포본이라 OFL Reserved Font Name 조건 3은 걸리지 않는다. 링크되는 코드: S8-2c(`9c2bf391`, 보강 `04f7d54f`)가 Rust 표준 라이브러리 1.98.1, std가 `x86_64-unknown-linux-musl`·`x86_64-pc-windows-msvc`에 링크하는 crate(in-tree `compiler_builtins` 0.1.160 포함), musl 1.2.5 `COPYRIGHT`, Microsoft WebView2 로더(SDK 1.0.3800.47, `.lib`는 NuGet 패키지와 바이트 동일)의 라이선스를 고지에 더했다. 원문은 표준 라이브러리 저작권 문서(`COPYRIGHT-library-in-tree.txt`, toolchain의 HTML을 일반 텍스트로 옮김)를 빼고 출처와 바이트가 같고(S8-2c 과제 검토 `a09401a4…`; `compiler_builtins` 원문 2개는 재검토 `acacc677…`) 사용자가 2026-11-04에 "예, S8에 포함 (권장)"으로 승인했다. 정적 Visual C++ 런타임의 Microsoft 조건은 확인하지 못했다(§4 문서·고지). 브랜치 전체 검토 I-1에 따라 설치 파일(`devbox-<버전>-setup.exe`와 설치 폴더의 `uninstall.exe`)에 든 NSIS 3.11(`COPYING`, Modern UI 2·NSISdl 라이선스)과 Tauri의 `nsis_tauri_utils` 0.5.3(MIT·Apache-2.0) 원문도 더했다(`566dc77b`·`9f9a7e3d`, 2026-11-04 사용자 결정의 원칙에 따른 판단, SDD 원장 Ruling 114).
- S8-3 때 코드 head `04f7d54f`의 전체 `pnpm check`는 **FAIL**이다(exit 1, 826.4 s, 2026-11-05 07:31–07:45 KST). Playwright만 1개 실패·22개 통과했고 나머지 단계는 모두 통과했다. 실패한 시험은 `e2e/terminal.spec.ts:51` "split panes persist and closing a pane keeps its session available"로, Devbox가 둘째 칸을 붙이는 동안 fixture의 tmux 서버가 비정상 종료했다. 원인은 tmux 버그 LP #2169164(제어 모드 클라이언트가 연결되는 중 `control_write()`에서 서버 SIGSEGV)이고 이 PC의 tmux `3.6a-2ubuntu0.1`이 해당 판이다(근거: controller SDD 원장의 S8-3 재실행 원인 항목. 실패 trace는 재실행이 덮어써 보존하지 못했다). Devbox 회귀가 아니다: `70cd633f`는 같은 tmux로 23/23이었고 그 뒤에는 xtask·고지·Markdown만 바뀌었다. 같은 SHA의 E2E 재실행(`bash scripts/verify-entry.sh e2e`, 132.6 s, `memory_enforced=true`)은 23/23 PASS지만 전체 검사를 PASS로 바꾸지 않는다. 826.4 s는 실패한 실행이라 SC7 측정이 아니다. 그 뒤 `bb33fdc2`·`2cebe463`은 어느 검사 단계도 읽지 않는 Markdown만 바꿨고, `2cebe463`의 전체 `pnpm check`는 PASS다(exit 0, 806.6 s, 2026-11-05 15:00–15:13 KST, `memory_enforced=true`, Playwright 23/23, tmux는 그대로 `3.6a-2ubuntu0.1`). 그 뒤 `566dc77b`이 고지 생성기와 고지를 바꿨고, S8-4 최종 검사 `5c84129a`는 PASS다(753.2 s, 줄이기 전 §1). README 요구 사항에 고친 tmux 판(3.7 이상, Ubuntu 22.04 `3.2a-4ubuntu0.3`·24.04 `3.4-1ubuntu0.2`·26.04 `3.6a-2ubuntu0.2` 이상)을 적었다.

## 4. 열린 후속

리뷰 상한 뒤 지적과 과제가 미룬 항목 가운데 고치지 않고 남긴 것이다. **[차단]**이 없으면 1.0.0을 막지 않는다. 출처·근거 경로·검토 판정은 이전 전문 §8([docs/history.md](../../../history.md))에 있고 ID로 찾는다. S1–S6 과제 기록(이전 전문 §5)의 "후속(비차단)" 메모는 옮기지 않았으니 docs/history.md의 같은 전문에서 찾는다. 닫으면 여기서 지우고 PR 본문에 근거를 적는다.

- **에이전트·하네스**
  - Codex 보내기(S7-5): tmux·pane 캡처 실패가 다음 행동 없는 internal 오류로 끝난다 · Idle pane의 마지막 6줄이나 화면에 패턴과 같은 글(기록에 남은 `/ D I F F`, 선택지 줄)이 있으면 화면이 바뀔 때까지 보내기를 거절한다(닫힘 쪽, 낮은 확률).
  - 알아보지 못하는 Codex 화면(Fix A 3회차): 끊긴 동안의 초안, "Input disabled.", `/`·`@` 목록이 열린 초안, ChatGPT 로그인, Enter 힌트 없는 짧은 로딩, 힌트가 잘리는 좁은 터미널, 키 재지정. 받아들인 과일치: 전체 화면 기록의 활동 살펴보기.
  - 알려진 한계(Fix A 4회차): pi 셸 모드(테두리 색만 바뀐다, 제품 결정), 좁은 터미널의 Codex `Shell mode`·/side 라벨, Codex 외부 편집기(Ctrl+G).
  - 보이지 않는 제어 문자가 든 글은 `agents.control_characters`로 거절된다(F2, 초안은 남는다). 후보: `quoteForPrompt`가 control picture로 보이고 글자 이름을 말한다.
  - 정확히 `exit`·`quit`·`:q`만 보내면 opencode가 끝나는 것으로 추정된다(F3, `-s <id>`로 재개). 정확한 낱말의 MCP 거절은 나중에 더할 수 있다.
  - bracketed paste를 켜지 않은 도구 상태에서는 여러 줄 글이 줄마다 제출되고 `!`·`/`로 시작하는 뒷줄이 MCP 거절과 상관없이 셸·도구 명령으로 실행된다(Ruling 58, SECURITY.md에 적었다).
  - opencode(S7-7): cost 합산 없음 · subagent 토큰 빠짐 · 사용자 `OPENCODE_CONFIG_CONTENT`를 데몬 환경 값으로 대체 · idle로 버린 요청의 문구 미갱신 · 루트 idle 뒤 자식 요청 남음 · 빈 PATH 항목 처리 차이.
  - opencode Fix B: env 옵션 없이 띄우는 `Process.spawn` 호출자를 다 세지 못했다(Bun Worker의 `process.env` 차이). 에이전트가 닿는 spawn은 모두 spread라 수정은 유효하다(S7-14 H3).
  - pi(S7-8): 사용자 `sessionDir` 사용량 미집계 · Codex 시작 창 경합 · `~/.pi/agent` lock(pi 자체 동작) · "확장 없음" 배너 오표시 두 경우(ready 보고 유실, setup 90초 초과) · pane 패턴은 pi 1.3.0 문구 기준.
  - 재개: session id가 없는 Claude Code·opencode 작업은 원래 지시로 새로 시작한다(S1부터). Codex는 rollout 기록으로 세션을 확인하지 못하면 재개하지 않고 `agents.session_metadata`로 끝난다(S8-4b-5에서 새 턴 없는 재개도 같은 thread로 이어지게 고쳤다).
  - 브랜치: 이름이 정확히 `agent`인 브랜치가 있으면 모든 `agent/…`가 막힌다(준비 단계 실패, 손실 없음) · 기본값과 같은 입력을 '고침'으로 보는 것을 고정하는 시험 없음 · 칸을 비웠을 때 설명 없음.
  - 준비: `[agent] copy` 실패가 다음 행동 없는 internal 오류로 끝난다(S7-11, SC5).
  - 환경(S7-14): 셸에만 있는 API 키는 에이전트 작업에 넘어가지 않는다(설계, [README "에이전트 도구"](../../../../README.md#에이전트-도구), `crates/cli/src/setup.rs` `ALLOWED`). 후속: 환경 점검이 셸에만 있는 `*_API_KEY` 이름을 알리고 각 CLI 로그인을 안내한다(값은 다루지 않는다).
  - 경로 찾기(S7-15 M-1): 조회 셸 `bash -lic`가 `~/.profile`·`/etc/profile.d`를 조회마다 실행해 로그인 파일이 띄우는 데몬(예: `ssh-agent`)이 조회마다 남을 수 있다(이 PC 미확인). 후속: [README "도구 경로 찾기"](../../../../README.md#도구-경로-찾기) 옆에 문서화한다(rc.2 릴리스 본문에는 적었다).
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
  - S6-6b native M1–M8: Unknown attempt가 세션 내내 설치 차단 · `update_install_*`에 `main_only` 없음 · 30초 timer가 `Cancelled` · 남은 취소 stage를 `InstallFailed`로 분류 · `disarm` 무한 대기 · `Prepared::commit` 재해시가 UI thread·락 안 · 다운로드 300초 고정 · 소문자 "devbox" 창 제목 · PR #653 독립 검토의 코드 후속(1.0 뒤 첫 patch 후보): 업데이트 runner가 새 앱을 `--minimized`로 시작해 창·알림이 없다(새 판의 시작 코드가 runner 기록 `Success`를 볼 때 창을 열면 1.0.0 → 그 판 업데이트에도 적용된다), runner가 NSIS에 `/S`만 넘겨 업데이트마다 바탕 화면 바로 가기를 다시 만든다(`/UPDATE`는 시작 메뉴 바로 가기와 WebView2 설치도 건너뛰므로 따져 본 뒤 넣는다), 제거가 자동 시작 Run 값 `io.github.jihoon22lee.devbox`를 지우지 않는다(NSIS는 `Devbox` 이름만 지운다).
  - `19cff21e` 검토 minor: 순서 assert가 문자열 일치뿐 · 사용자 명령이 exit 259로 끝나면 identityMismatch가 이어지는 모호함.
  - 수용 harness: A6은 uninstall 등록 key 제거를 bounded 대기한 뒤 판정 · A5는 vault 경로가 bundle 입력에 닿는 fixture와 검토된 UI helper 뒤 실행 · A3는 단축키 등록 증명 helper를 검토받은 뒤에만 실행.
  - 수용 보고: 사용자의 v0.6 `wsl-desktop.exe`가 `%LOCALAPPDATA%\Devbox\integration\**`에 계속 쓴다. v0.x 제거 전까지 "v1 prod unchanged; legacy v0.x external writer changed N file"로 보고한다. 그 밖의 prod 차이는 FAIL이다.
- **앱·설정**
  - setup이 파일을 쓴 뒤 재시작 전에 끊기면 다음 자동 시작이 재시작을 생략한다([다시 설정]으로 복구).
  - `api.redaction_failed`(matcher)의 다음 행동이 약함 · 새 설치에서 [나중에]가 `autostart:true`를 기록(무해) · `redactKnownTokenPatterns`의 `sk_`·`sk-`에 왼쪽 경계 없음(데몬과 함께 고친다) · 노트가 꺼진 동안 search.notes 색인이 낡음 · `SupportBundle`의 `notes_disabled`에 다음 행동 없음.
  - 영역 5 M6: 1 MiB를 넘는 로그 tail은 자르지 않고 실패한다. M7: bridge 재연결 루프가 200 ms 바닥에서 돌 수 있다.
  - Fix E: [다시 시도]로 깨진 `settings.json`에서 회복해도 다시 열 때까지 전역 단축키 미등록·트레이 옛 오류(M2) · 자동 시작 설정 명령이 설정 오류 원문을 보임 · `refused`가 Welcome 뒤에도 남음(nit).
  - Fix D §6: 몇몇 화면이 오류 제목만 보인다(`pasteImage.ts`, `ReadOnlyBar`, `ActivityDay`, `ActivityWeek`, `FileOperationDialog`, `FileTree`, `SourcePage`, `QuickOpen`). `RunTab.tsx`는 다음 행동이 없는 코드에서 "undefined"를 찍는다(`messageText`로 고친다).
  - Fix F: `config.local.toml`의 나쁜 규칙은 설정 → 활동에서 저장해도 다음 시작까지만 기록이 재개되고 편집기는 그 파일을 말하지 않는다. 편집기는 합친 `[activity]`를 보이지만 다섯 키를 모두 `config.toml`에 쓴다. Fix F(선택): 읽기 전용 미리보기도 tmux 서버 버전을 묻는다 · 규칙을 읽지 못하는 동안 Windows 수집기가 앱 이름 전송을 멈출 수 있다(데몬은 저장하지 않는다).
  - `projects.in_use` 안내의 "열린 작업" 모호함(`agentMessages.ts`, S6-6b front M3 잔여) · OAuth 승인 배너가 승인 호스트를 보이지 않음(front M4, `RequestEditor.tsx`). 실행기 패널의 OAuth 승인 연결(O7)은 1.0 이후다.
- **API·노트**
  - S6-6b api M3: 장시간 MCP-HTTP/SSE 세션이 Set-Cookie seed를 쌓아 약 1,000건 뒤 `api.redaction_failed`로 끝날 수 있다. M4: 잘못된 Basic credential의 디코딩된 비밀번호는 seed되지 않는다.
  - S6-6b docs M1–M6: 영수증 파일 미정리 · holder 거절 뒤 Attempted 잔류 · `notes.select_response_target`이 query인데 상태를 바꿈 · 취소·stale rename 캡처 무응답 · 캡처 BFS가 `target/`·`node_modules/`·`.git/`을 건너뛰지 않음 · 끝나지 않은 proof가 다음 rename을 30초 막음.
  - 오늘 한 일 요약([오늘 한 일 넣기])에 활동 합계가 없다(01-design §2.2와 다름, `summary.rs:202–227`) — 1.0 뒤 기능 또는 설계 정정
- **시험·크기**
  - 시험이 `/tmp/tmux-<uid>`에 socket을 남긴다 · tmux.conf와 `apply_extended_keys`의 옵션 중복, format 줄 시험 없음.
  - 거의 꽉 찬 파일(2026-11-02 `wc -l`): `execution.rs` 600/600, `TerminalView.test.tsx` 592, `cli/tests/agents.rs` 591, `config/lenient.rs` 590, `windows_observation.rs` 573, `terminal/src/tmux.rs` 572, `prepare.rs` 567, `discard_snapshot.rs` 556, `command_path.rs` 542. 다음 추가에는 나눈다.
  - T1b: ignored systemd wrapper 시험 4개(`runtime_secrets.rs` 등)는 owned-copy 시험 seam이 없어 실행해도 통과할 수 없다(NOT_RUN). ignore 사유도 실제 원인을 가린다.
- **문서·고지**
  - 확인하지 못한 조건 둘: 정적으로 링크된 Microsoft Visual C++ 런타임(Tauri 기본 `staticVCRuntime`, UCRT는 동적)에 붙는 Microsoft 빌드 도구 조건, 설치 파일에 든 NSIS LZMA 압축 모듈(CPL-1.0과 NSIS `COPYING`의 예외)의 목적 코드 배포 조건. 고지에는 사실만 적었다(`04f7d54f`·`566dc77b`). S8-5 승인 때 알린다. VC 런타임 문장(`staticVCRuntime`)과 설치 파일 문장(NSIS 압축 기본값, 사용자 NSIS 템플릿·hook·`fileAssociations` 없음)을 `tauri*.conf.json`에 묶는 시험이 없다 — 1.0 뒤 첫 코드 PR에서 더한다(브랜치 전체 검토 M-8). `fileAssociations`를 켜면 라이선스가 없는 `FileAssociation.nsh`(gist)의 매크로가 설치 파일에 들어간다
  - musl 파일별 BSD 원문: rc.2와 1.0.0 후보(`1cc2e28d`)의 `devbox-linux`는 `libc.a` member 1,335개 중 354개를 링크하고(S8-4 대조 스크립트, S8-2c 검토의 셈은 1,332개 중 351개), 그 가운데 파일별 라이선스가 붙은 musl 소스(22·56·14개 목록)는 없다. 링크되는 member가 바뀌면 다시 본다(S8-4에서 1.0.0 후보로 다시 확인: 목록 파일 0개)
  - xtask 주석 두 곳(`xtask/src/notices/linked.rs:24-27` in-tree 행 조건, `:58-64` '사용자 질문' 문구)은 1.0 뒤 첫 코드 PR에서 다듬는다(S8-2c 재검토 Minor)
  - 고지에 라이선스 파일이 없는 패키지 21개(`@tauri-apps/plugin-opener`는 `LICENSE.spdx`만) · `COPYRIGHT`·`UNLICENSE`·`LICENSES/`는 이름 규칙 밖이라 읽지 않는다.
  - 고지 지문 밖의 입력 둘(생성기 코드 `xtask/src/notices*`, 글꼴 파일 자체): 바꾸면 `cargo run -p xtask -- notices`를 직접 돌린다.
  - 옛 README.md:153의 "성공 기준 SC2·SC6·SC10"(사용자 문서에 남은 내부 ID, Ruling 72)은 S8-2b가 개발 절을 옮겨 지금 [app/README.md](../../../../app/README.md) "Devbox 화면"에 있다. README·guide·config·SECURITY에는 SC ID가 없으니 닫을 후보다.
  - 옛 원장 절을 가리키는 코드 속 표시 두 곳(`app/src/features/tools/api/lib/storage.test.ts:439` 시험 이름의 "PROGRESS S5-3a·3b 후속", `crates/lsp/src/servers.rs:13` 주석의 "PROGRESS S4-3")은 1.0 뒤 첫 코드 PR에서 [docs/history.md](../../../history.md)를 가리키게 고친다. 제품 코드라 S8에서는 그대로 둔다. 환경 점검 문구 두 곳(`crates/tools/src/doctor/probe.rs:59` 주석, `crates/tools/src/doctor/agent_tools.rs:36` 경고의 "devbox가 확인한 가장 낮은 버전")도 README와 맞춰 "연결 형식을 맞춰 본"으로 고친다(브랜치 전체 검토 M-1).
  - `crates/cli/src/daemon/summary.rs:4` 머리 주석(활동 합계 합류 예정)은 실제와 다르다 — 1.0 뒤 첫 코드 PR에서 고친다.
- **성능·CI·릴리스**
  - SC2: 공개 전 반복 cold/warm 측정과 시작 경로 비용 분해 뒤 수정하거나 명시적 미달·다음 patch 계획을 정한다(S6-6b A2 raw만 있다).
  - SC7: Windows job 시간 분해와 ci-docs M4(musl release 시험이 모든 PR의 `linux-bin` → `Rust (Windows)` 임계 경로에 있다, 미계측). 필요하면 tag/dispatch에만 돌린다.
  - ci-docs M2: release publisher의 stable 경로는 첫 실제 실행이 S8-5다. scratch repo dry run은 실제 GitHub 대상이라 사용자 명시 승인이 필요하다. N2: x509(`-----BEGIN SIGNED MESSAGE-----`) 태그 서명이 릴리스 노트에서 지워지지 않는다.
  - #649 M2·M3: release 시험이 어느 검사가 거절하는지 고정하지 않음 · rc receipt-version 불일치 시험 없음.
  - Dependabot의 npm 보안 갱신 job이 반복해서 실패한다(seroval·katex·`@vitest/mocker`·undici, 원인 미조사, 1.0.0 뒤). S8 PR 머지 뒤 경고 7건이 닫혔는지 확인한다.
- **Windows·에이전트 실기(S7-14 NOT_RUN, 실행하지 않으면 PASS가 아니다)** — 정본은 [계획 S7-14 표](10-s7-s8-release.md#s7-14-windows-후보accept-수용), 실행 결과는 이전 전문 §5 S7-14 표다.
  - 2026-11-08 WSL 직접 시험(`70b1b4f6`, §3 2, 실행 기록 `devbox-verification/s8/harness-1.0.0/wsl-fresh/triage-run-70b1b4f6-*.md`): claude 행의 시험 도구 수정은 여기서 멈췄다(Ruling 157). claude NOT_RUN 행은 H1X-SHELL (a)(d)(시험 도구가 입력·`C-u` 4 ms 뒤에 찍어 실패, D14. 제품 쪽 (b) 셸 모드 보내기 거절은 PASS), H1X-HELP(`?` 도움말 대신 셸 모드 화면을 기록), P1(셸 모드가 남아 제품이 승인 지시를 거절했다. 사용자 Claude가 auto mode라 고쳐도 승인 화면이 안 나올 수 있다), SN2(부분: claude·codex 쪽은 대기 중 보내기를 모두 거절, pi·opencode 쪽은 진행하지 못함)다. codex H2X-SCREENS는 부분이다(이름 바꾸기·상태줄·키맵·재개 목록은 PASS, 질문 화면·인라인 기록은 Codex가 열지 않아 NOT_RUN). 환경 NOT_RUN은 CL3(GUI 도우미 없음)·P1X(사용자 Codex 설정의 YOLO 정책)다. harness: D11 H1X-TRUST 없이 돌면 claude 블록 `main()`이 신뢰 화면을 한 번만 찍는다(`trust_row`처럼 기다려야 한다) · D12 `?`·`!` 초안과 도움말을 닫는 Escape를 입력창 확인 없이 보낸다 · D13 P1의 NOT_RUN 사유 문장이 사용자 설정의 승인 없는 허용과 이미 끝난 도구를 가르지 않는다 · D14 H1X-SHELL이 입력·`C-u` 직후 4 ms에 화면을 찍는다 · D15 H1X-SHELL 뒤 셸 모드를 끄지 않는다 · fix2 검토 메모: `C-u` 뒤 다시 찍지 않음, `screen_text`가 `›` 줄을 모두 버림(H2X-FEEDBACK (c)는 0.167.0에서 증거가 아니다), D7의 Down 뒤 화면 배치 추정은 `8ad0ac09`·`a1667181`의 실제 화면에서 맞았다(닫힘), NOT_RUN 사유 문장이 codex 0.166.0을 말함, H2X-SHELLMARK `/side`·H3X-ENV는 입력 3 ms 뒤 Enter(다음 전체 `wsl-fresh` 전에 고친다). S8-5 Windows 확인이나 1.0.x에서 다시 돌린다.
  - K1: tmux 3.4 호스트와 3.5 이상 호스트의 Shift+Enter(pi 줄바꿈 포함), 3.5 경계, WebView2, 호스트 tmux를 올린 뒤의 동작(K1X-TMUXUP은 Q3 미승인).
  - H1: Claude 폴더 신뢰 대화가 새 작업 폴더에서 뜨는지, 셸 모드·`?` 도움말 실화면. H2: Codex 재개, `codex resume` 직후 1초 안 보내기, `/feedback`·실화면 목록, 외부 편집기.
  - H3: 실제 opencode·pi, opencode 환경(`DEVBOX_AGENT_ID`·`DEVBOX_AGENT_LAUNCH` 없음)·중첩 `opencode run`·셸 모드 힌트·`exit`/`quit`/`:q`의 효과.
  - pi 행(H4, K1X-SEND, SN2의 pi 부분): Q6 뒤 pi가 쓸 수 있는 모델을 가지면 다시 돌린다. 시작점은 harness 검토 7 Note B(재시도 대기 중 idle 보고 여부).
  - P1: 실제 Claude 승인 상관과 Codex exec 승인. U1: rc.1 Claude 작업의 승인 뒤 보내기 막힘과 재개 회복.
  - W1 추가: 빌드 서버가 사본을 붙잡는 시간과 문구, `#[ignore]` bootstrap 시험, busy 없는 정지·수정·재시작, 콘솔 창 없음, cold 시작 35초, 느린 holder 뒤 업데이트 설치(W1 (다)는 Q4 미승인). readiness의 direct/native 동등성(S6-4a `313a6095`).
  - 연결·설정: STOP한 데몬에서 앱 안 회복, 강제·빠른 재시작의 MainPID, [나중에], [자세히], `settings.json` 깨짐, 느린 정상 시작의 [다시 시도]. O1a·O1b: Windows 실제 재시작·연결 유지. O6b: `mark_autostart_asked`.
  - 정리·버리기: 실제 정리 명령, 스쿼시로 닫은 작업의 실패한 정리, 하위 모듈 작업 폴더(CL3 부분, CL4–CL6).
  - Windows 절반: CS3(WSL 절반은 통과, Windows 절반은 harness D4), CS5(부분 a 통과, hook 절반은 Q1), S6 묶음 G1·G2(harness D1) 재실행. C1·M4·N1 accept 고지 파일·A1·A2·A4·A6은 `70b1b4f6`에서 PASS였고 최종 후보에서 다시 한다.
  - S7-11 문서의 확인 대기 자리: 서명 없는 설치 파일의 Windows 경고(SmartScreen), 앱 제거 뒤 남는 항목(NSIS 제조사 key 포함, 대화형 제거 미확인), MCP 등록 명령의 설치본 재확인.
  - S8-1b(zip 9.0.5): 설치본에서 지원 번들을 쓰는 것을 포함한 Windows 실행은 NOT_RUN이다(이전 전문 §5 "Dependabot 보안 경고 7건과 S8 의존성 갱신").
- **운영**
  - 2026-10-22 WSL 재시작 뒤 `user@1000.service` 실패로 메모리 cgroup이 걸리지 않았다(`memory_enforced:false`). 그 뒤 검증은 자식 환경에만 `XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`를 줘 `memory_enforced=true`를 확인하고 돈다(호스트 서비스는 바꾸지 않는다). 걸리지 않으면 available 메모리가 2 GiB 아래일 때 무거운 빌드를 멈추고 실행 결과에 미적용을 적는다([CONVENTIONS §5](../../../../CONVENTIONS.md#5-검증), 사용자 결정 2026-10-22).
  - tmux 3.7 전 판의 제어 모드 연결 중 서버 SIGSEGV(LP #2169164): 2026-11-05 이 PC(tmux 3.6a-2ubuntu0.1)의 E2E(@04f7d54f)에서 일어났고, S1 CI의 tmux 3.4 서버 비정상 종료(`c68c923e`, 재실행 통과)도 같은 결함일 수 있다(미확인). 호스트 tmux 결함으로 보고 고친 판을 README 요구 사항에 적었다. 사용자가 2026-11-05에 이 PC의 tmux를 `3.6a-2ubuntu0.2`(고친 판)로 올렸다(이미 떠 있던 tmux 서버는 다시 시작할 때까지 옛 코드로 돈다). Devbox 쪽 완화(환경 점검 경고, 연결 중 알림을 만드는 명령 줄이기)는 1.0 뒤 첫 patch 후보이고 재시도 래퍼는 쓰지 않는다.
- **1.0.x 후속(최종 교차 검토·S8-4b 검토·rc.3 검토·S8-5b 검토)** — ID와 짧은 이름만 적는다. 내용은 통합 보고 `final-report.md` §4·§6, rc.3 단계별 통합 보고 `rc3-review/known/w1-report.md`·`w2-report.md`·`w3-report.md`와 SDD 원장(§6 근거 보관)에 있다. rc.3 검토 ID는 단계 번호를 앞에 붙인다(`1:X1-01`, 계획 10 §S8-5b).
  - B(사용자가 1.0.0에서 뺌, 2026-11-06): X2-04 빠른 열기의 추적하지 않는 새 파일 · X3-04 컬렉션 요청 클릭 뒤 편집기로 이동 · CB-04 실행 시작 검사 원인별 문구 · CA-04 Windows 실행 오류 원문 · CB-06 세션 없는 Claude Code·opencode 재개 확인 창 · CB-03 최소안 상태 알림 점검 행 문구 · X2-01 숨긴 변경만 있는 작업 폴더 자동 정리(위 폐기·정리의 "(기존)" 줄). 계획 승인에서 뺀 CB-02: 한글만 있는 제목의 브랜치 이름.
  - D: X1-04 `runs/` 30일 정리 없음 · X2-06 go.sum 다른 판 거짓 양성 · X2-03·CA-03의 코드 수정(문서는 README 알려진 제한에 적었다) · Minor는 통합 보고 §6 표 · M-1 `/UPDATE`(위 Windows 줄의 PR #653 코드 후속).
  - S8-4b 과제 검토에서 남긴 것: 4b-1 서로 겹치는 두 비밀의 꼬리 · 4b-2 바깥 진단 거르기를 한 번에(지연만) · 4b-3 예약 실패 알림의 run id, `[launch]` 두 번째 실행에서 마지막 root만 다시 실행, 준비 조건이 바뀐 재사용 의존의 오류 안내 · 4b-5 inline Codex 작업이 작업 폴더를 같이 쓸 때 discovery가 다른 작업 세션을 묶을 수 있음(기존 동작), tracker 읽기 한도가 맞은 후보만 셈 · 4b-6 [취소] 뒤 일반 실패 문구(트레이·업데이트·팝아웃 `blocked`, native ACK에 거절 이유가 필요하고 1.0.0에서 처음 업데이트할 때도 나온다), 손대지 않은 [새 요청] 탭도 물음(판별은 선택), 30초 기한 안내가 문서만 말하고 두 확인 창이 30초를 나눠 씀 · 4b-7 같은 이름 환경 변수 경고 없음 · 4b-10 유지 연결의 `wsl.exe`를 runner 폴더보다 System32에서 먼저 찾기(앱 브리지 `platform/wsl.rs`도 같음), 유지 연결을 끝낼지 남길지 판단(Ruling 126)의 자동 시험 없음 · S8-4b-12 전체 검토 M1 의존 재사용의 핵심 분기가 ignored 시험에만 묶임(CI용 `start_graph` 시험이나 예약 실행), M2 업데이트 확인 창의 WSL 유지 문장이 조건(누를 때 연결 없음·띄우기 실패·900초를 넘는 설치, 설치 시한은 20분)을 말하지 않음(설치된 판의 문구).
  - rc.3 1단계(결함): 1:X1-01 의존 서비스 재사용이 worktree를 보지 않음 · 1:X2-01 같은 폴더의 다른 Codex 대화를 묶음(코드, 위 4b-5와 같다, 문서는 README 알려진 제한) · 1:X1-03 예약 행이 예약 id로 실행을 찾음 · 1:X2-04 쿠키 저장·삭제 경합 · 1:X3-04 충돌 뒤 늦은 다시 읽기 · 1:X3-05 첫 프로젝트 자동 선택 경합 · 1:CB-05 응답 노트 붙이기 오류 이유가 뭉개짐 · 1:CB-06 CLI가 오류 코드만 찍음 · 1:CB-07 끊긴 동안의 알림이 복구되지 않음 · 1:CB-08 진단 ID 없는 internal 안내 · 1:CB-09 문제 목록 페이지 경합 문구 · 1:X2-02 Codex 안 `/resume`·`/new` 뒤 완료 보고·[재개]가 처음 대화에 묶임(코드는 새 시험 행과 함께, Ruling 159, 문서는 README).
  - rc.3 2단계(사용성·기능 연계): 2:X3-01 하단 터미널 전역 단축키 · 2:X3-02 Windows에서 연 알림의 읽음 처리 · 2:X3-04 설정 기능의 팔레트 바로가기 · 2:X1-03 로그 소스를 이름으로 추가 · 2:X1-04 로그에서 내장 편집기 열기 · 병합 기준 체크아웃 사전 검사와 충돌 중 재개(2:W2-CA-01의 나머지) · 나머지 38건(`known/w2-report.md`, 원 보고의 "넣을 때").
  - rc.3 3단계(디자인): 3:X1-05 하단 로그 이중 스크롤 · 3:X3-03 긴 프로젝트 이름이 제목 줄을 밂 · 3:X3-05 UI 배율과 API 좁은 배치 · 3:X2-04 '오늘 한 일 넣기' 문구(2:X2-09와 함께) · 3:CB-07 밝은 테마의 터미널·편집기 색 대비(Ruling 158) · [출력 색상 끄기] · 실행 목록 행 강조 · 활동 막대 세로 표시 · 나머지 35건(`known/w3-report.md`).
  - S8-5b 과제 검토에서 남긴 것: 5b-1 `hold_start` 계산량 상한 없음(긴 반복 비밀·많은 패턴, 지연만), 경계를 넘는 일치 뒤 바이트가 한 번 더 기다림, API `StreamRedactor`(`crates/api/src/redact.rs`)의 고정 보류, 늘어난 `pending`의 옛 할당과 `copy_stream`의 8 KiB `chunk`를 지우지 않음(기존) · 5b-2 시험 이름 `preserves_code` · 5b-3 정의가 없어진 살아 있는 일반 실행이 목록에 없이 `projects.remove`를 막음 · 5b-5 대기 중이거나 붙여넣기가 실패하는 동안 온 Claude `SessionStart`(resume)를 건너뜀, Codex 준비 화면의 부정 사례가 실제 0.167.0 캡처가 아님(S8-5b-28b H2가 실제 작업 화면을 남기면 닫는다), 보내지 못한 보내기 뒤 이미 본 준비 화면을 다시 그릴 때까지 보류(실행 중으로 남음), `watch_tests.rs` 584줄(다음 추가는 새 모듈), opencode·pi 재개 뒤 상태(바꾸지 않음, 실제 도구 근거 없음) · 5b-5b opencode export 잠금 대기와 닫기의 action lock(지연만) · 5b-6 팔레트 명령이 초점을 둔 자리를 없애면 초점을 잃음 · 5b-7 본문·바닥줄 구분선 없음, pre-line 시험이 CSS를 정규식으로 읽음, 깨진 배율 값 복원 시험 없음, DefineDialog [닫기]·[저장] 줄이 폼과 함께 스크롤, UpdateConfirmation·DocumentCloseDialog·FirstRun의 `focus()`에 `preventScroll` 없음, 스크롤 목록에 `min-height` 없음 · 5b-8 `Button` 밖의 `aria-pressed` 단추(CodeEditor 미리보기·QrTool·JsonYamlTool·SearchResults) · 5b-12 머리 [새 작업]이 본문 주 동작 옆에서도 primary · 5b-14 데몬이 죽은 뒤 닫힌 작업의 지시가 늘 '보내는 중' · 5b-16 무거운 `git.status`(git 9–10번, 추적하지 않는 파일이나 diff가 8 MiB를 넘으면 계속 실패), 기존 Git 감시 비용(대상마다 100 ms 걷기, 지운 작업 폴더도 계속, 오래된 감시를 지우지 않음) · 5b-18 앱 편집기 밖 경우·lsp 낡음 검사 시험 없음, lsp 갈래의 열기 호출 중복, lsp 문제를 현재 프로젝트로 좁히기(데몬, Ruling 181) · 5b-19 저장한 제안이 목록에서 빠지는 것을 시험이 직접 보지 않음 · 5b-20 실패 로그 공용 문구(`failureLogs.ts`의 '로그 페이지가 반복되어 전송하지 못했습니다.', RunTab의 '요청이 취소되었습니다. undefined' — 위 Fix D §6 줄의 취소·시한 경로, Ruling 185), 정의가 없어진 실행과 실행만 가리키는 링크에서 읽지 않은 지난 실행을 고르면 선택기 초점을 잃음 · 5b-21 비밀 행에 값을 저장한 뒤에도 입력란이 친 글을 보임(파일은 빈 값) · 5b-22 종료 문서 준비 중(봉인 전)에 쓴 빠른 캡처 초안, `service_tests.rs` 591/600줄 · 5b-23 Ctrl+Shift+T `TerminalCommands` catch 시험 없음, 리스너 effect의 낡은 `exit` 의존(무해), 모르는 작업 수와 초안을 함께 보는 시험 없음 · 5b-24 `projects.list`를 읽는 동안 이름 대신 프로젝트 id가 보임(M4) · 화면 확인(Ruling 165) RC3-SHOT-05 idle 작업만 있을 때 빈 본문에 동작 없음, -06 '입력 대기'가 두 집합을 셈(용어 결정), -07 ANSI 배경 위 글자 대비, -09 밝은 테마 초점 링 2.2:1(앱 전체, 기존), 긴 이름 [도구] 목록이 150%에서 창 제목을 덮음 · forced colors에서 초점이 보이지 않는 세 곳(`notes.module.css:52-56`, `DefineDialog.module.css:31-34`, `source.module.css:197-201`, 기존).

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

- `devbox-verification/`: 실행 근거. S7은 `s7/`(`harness/`, `runs/<실행>/`, `release-v1.0.0-rc.2/`, `windows-run-rc2*/`, `windows-run-1.0.0-70b1b4f6/`(`70b1b4f6` 산출물, harness가 읽는다), `tmp-archive-2026-11-03/`). S8은 `s8/`(`windows-run-1.0.0-<SHA>/` 산출물 대조, `final-review/` 최종 교차 검토의 지침·보고·`final-report.md`·`SHA256SUMS`, `sdd-10-s7-s8-release/` S7·S8 SDD 원장, `rc3-review/` rc.3 세 단계 검토의 지침·원 보고(`w1`–`w3/<ID>/r1.md`·`r2.md`)·단계별 통합 보고와 범위 초안(`known/`)·화면 캡처(`shots/`), `harness-1.0.0/` 1.0.0 후보의 harness 수정·검토·실행 triage(직접 시험은 `wsl-fresh/`), `windows-run-1.0.0-70b1b4f6-logs/`·`w1-ra-*-70b1b4f6.log`·`interop-diagnosis-2026-11-0*.md`(S8-5 사전 확인)).
- `devbox-archive/`의 압축 보관. 기록의 `devbox-verification/...` 경로는 보관 안의 같은 상대 경로다.
  - `local-verification-20261021.tar.gz`와 `local-verification-20261021.manifest.json`(2026-10-21 정리).
  - `local-verification-20261025.tar.gz`(S3–S5, SHA-256 `cb08876abd0a863c653263369a03eeb65d0548673999cae2b8c72cc03bfc1dbf`)와 manifest. 정리 근거는 압축 밖 `cleanup-20261025/`.
  - `cleanup-20261026/local-verification-20261026.tar.gz`(S6·RC, SHA-256 `a292415367811796f7f16efcf5ce2b02eb69be4e2e76b08ca0d1e67ea8afecea`)와 manifest·`deletions.json`·`archive.py`. S6 worktree 원장은 `wt-s6-sdd/sdd/`, RC는 `wt-rc1-sdd/sdd/`, 임시 파일은 `tmp-s4acc/`·`tmp-s6post/`.
- 과제 작업 공간(SDD)과 Ruling 근거: S7·S8은 `devbox-verification/s8/sdd-10-s7-s8-release/`(장부 `progress.md`, 지시문·보고·검토, Ruling 195까지·태그 노트 초안 `s8-3-tag-notes-draft.md`(1.0.0)·`rc3-tag-notes-draft.md`(rc.3))다. S8-4 정리 때(2026-11-05) `v1/s8-release` worktree의 `.superpowers/sdd/10-s7-s8-release/`에서 옮겼다(S7-15 선례). 이 위치와 S8-4 결과는 #653의 검증 SHA를 바꾸지 않으려고 다음 브랜치(`fix/v1-final-review`)의 문서 커밋으로 넘긴 기록이다(Ruling 91, CONVENTIONS §8 최종 SHA 원칙).
- 다음 세션은 manifest와 보관 파일을 확인하고 필요한 근거만 읽는다.

## 7. 최근 세션

최근 10줄만 둔다(§0). 그 전 줄은 이전 전문 §6([docs/history.md](../../../history.md))에 있다. 줄 안의 § 번호는 그 줄을 쓸 때의 구성이다.

| 날짜 | 하네스 | 한 일 | 남긴 상태(브랜치·커밋·다음 단계) |
|---|---|---|---|
| 2026-10-21 | Codex 하네스 관찰 UI 구현 | Task16 승인 설계 누락 보완: 프로세스 우선 목록·종료 확인, 포트 브라우저·소유자 종료. RED Rust1/UI3→직접 고유33개(프런트21·Rust12) PASS | `PortInfo.startTimeMs`는 fd 스캔 전후 시작 tick 일치에서 생성, 미확인/container는 종료 불가. 확인 snapshot 고정·stale 오류 후 갱신 완료까지 잠금/새 확인 회귀 PASS. 생성/타입·독립 검토·완료 Biome350 PASS, 미해결0. 제품21/문서2/artifact54 해시 일치, Biome0.896초·cgroup122,421,248B·swap0. 완료 커밋·문서 해시는 root-completion.json. 전체 검사·SC9·Windows·CI·PR 미실행. 근거 `devbox-verification/s2-task16/observe-ui/{summary,source-freeze,test-names,commands}.json` |
| 2026-10-21 | Codex 하네스 통합 수정·별도 브랜치 검토 | Task16 첫 전체 검사 실패 보존→직접140개·strict Clippy·E2E3 수정 PASS, SC9 CPU0.233331%/RSS39,899,136B PASS | 제품75개 동결·독립 검토 PASS·미해결0, 규모52,174줄·scripts1,500줄. 완료 Biome/커밋 뒤 최초 push·동일SHA Windows accept 수용·단일PR. 현재 설치 수용·CI·리뷰 미실행, S2 수용 완료 아님. 근거 `devbox-verification/s2-task16/` |
| 2026-10-21 | Codex 하네스 PR #642 1회차 수정 | Windows CI69개·Prod/Accept 패키징 PASS, 설치 전 errno110 FAIL·실기 미실행. 코드 리뷰 scheduler1건·CI 세션 assertion1건 보완 | 직접26개·영향 strict Clippy·별도 검토 PASS. 실제 CI 오류 UNKNOWN·원본 실패 보존. 첫 보안 리뷰 지적0건 완료·Biome 뒤 묶음 push, 최종 필수CI/후속 리뷰/머지 대기. 근거 `devbox-verification/s2-task16/pr/` |
| 2026-10-21 | Codex 하네스 PR #642 2회차 수정 | 코드3건·보안0건 판정, readiness 귀속/선택 프로젝트/비밀키 DB 실패 복구 및 CI 정리·터미널 준비·로그 flush 보완 | 직접 고유95개·영향 strict Clippy·독립 검토 PASS, 실패 원본 보존. 완료 Biome 뒤 묶음 push, 최종 CI·3회차 리뷰·설치 시도·squash 대기. 근거 `devbox-verification/s2-task16/pr/round2/` |
| 2026-10-21 | Codex 하네스 PR #642 3회차 수정 | b24d99d5 Linux/Frontend/E2E CI PASS, 코드1건·보안0건 완료. 다른 등록 프로젝트 cwd의 Run 귀속 우선순위 보정 | RED→직접2개·독립 검토·영향 strict Clippy PASS. 자동 리뷰3회 상한 도달, 완료 Biome 뒤 최종 SHA CI·설치 시도·squash 진행. 4회차는 차단 기준만 적용 |
| 2026-10-21 | Codex 하네스 S2 머지·인계 | #642 → squash1e82e649, 최종f703 CI3/E2E PASS·리뷰3회 처리. 최종 설치 사전 검사 errno110 FAIL·실기 미실행 보존 | 검증tree동일. 사용자 후속 요청으로 결과 문서 PR·근거 압축 보관·로컬 정리 진행, 다음 세션은 깨끗한 main에서 시작하도록 인계 변경. 상한 이후 Windows 준비 지적1건 §8 이월, S3/S5 제품 미착수. 정리 완료 여부는 실제 Git/PR·manifest로 확인 |
| 2026-11-02 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S7-13 마무리(영역 6 재검토 Ready, 버전 `1.0.0-rc.2` `22731033`, `pnpm check` PASS 1,508.6 s)와 S7-14: Windows 후보 `22731033`(run `41374497823` FAIL) → `240430cc`(run `41377466587`) → P-DEF-1 수정 `40d97a83`(run `41423905881` SUCCESS), harness 검토 6건·수정 4회차, WSL 수용 실행 4개(최종 `40d97a83-a4e9f78c` PASS 7행·FAIL 0, 업그레이드 `40d97a83-f6fd79ec`)와 NOT_RUN 이유(Ruling 90). 23:08에 세션이 다시 떴으나 WSL은 재시작되지 않았고(interop 여전히 `accept4 failed 110`) 사용자 지시로 WSL 재시작 요청(Q0)을 철회하고 남은 행을 NOT_RUN으로 두고 진행하기로 했다. 결과 기록은 patch(`s7-14-docs.patch`)로 만들어 S8-0으로 인계했다(Ruling 91). 23:55 KST에 PR #651을 열었다 | `v1/s7-rc2` `40d97a83` push, PR #651 열림(자동 리뷰 꺼짐 판정 창 10분). 다음: 독립 검토 1회·CI·머지. 근거 Git common-dir `devbox-verification/s7/` |
| 2026-11-03 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S7-15: 독립 PR 검토(CHANGES REQUIRED, Critical 0·Important 2·Minor 1)와 수정 두 건(`4c95c932` tmux 3.4 시험, `400a737f` I-2 `[agents]` 실패 정책, Ruling 93·95), 최종 head `400a737f`의 CI·수동 Windows run `41536487871`·재확인 실행 `400a737f-11d1bb99`(binding 검토 7 REFUSE → 31 case APPROVE, Ruling 96), PR #651 squash 머지(12:45 KST → `842fa82e`), tree 대조·main CI·S7 worktree/브랜치/`/tmp` 정리. S7-16: 사용자 승인 뒤 `v1.0.0-rc.2` 태그·prerelease 공개(17:35–17:57 KST, release run `41604456654`, asset SHA-256 세 곳 일치, `releases/latest` `v0.9.0`). 머지 뒤 Dependabot 경고 7건 → 사용자 승인으로 S8에 포함. S8 착수(Ruling 97: 사용자 확인 결과를 기다리지 않는다): worktree `v1/s8-release`, S8-1 `a6f75a68`(버전 `1.0.0`)·S8-1b `eb08e39b`(zip 9.0.5·seroval 1.7.7)·S8-0 `cfa89f0a`(S7 결과 기록) | `v1/s8-release` `eb08e39b` 위에 S8-0 문서 커밋, push·PR 전. 다음: S8-2 문서 통폐합 → S8-3 검사·후보 → S8-4 PR → S8-5 공개(별도 승인). 근거 Git common-dir `devbox-verification/s7/`, 작업 원장은 이 PC의 `.superpowers/sdd/10-s7-s8-release/` |
| 2026-11-04–05 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S8-2 문서 통폐합 끝: 과제 검토와 수정·재검토 회차(S8-2a 수정 `70cd633f`, S8-2b 수정 `82b48ea4`)를 마쳤다. 고지 두 건 사실 확인 뒤 사용자 답: 링크되는 코드(Rust 표준 라이브러리·musl·WebView2 로더)는 "예, S8에 포함 (권장)", 글꼴은 NAVER 공식 파일과 바이트 동일이라 변경 없음. S8-2c 고지 `9c2bf391`·보강 `04f7d54f`(과제 검토 Needs fixes → 수정 → 재검토 Approved). S8-3: `pnpm check` `70cd633f` PASS 1,267.4 s, 최종 코드 head `04f7d54f` FAIL(tmux 버그 LP #2169164로 Playwright 1개 실패, 같은 SHA의 E2E 재실행 23/23 PASS). Windows 행(accept 설치 스모크·H1)은 interop 고장(`accept4 failed 110`, 2026-11-05 07:32 KST 재확인)과 Q1로 NOT_RUN | `v1/s8-release` `04f7d54f` 위에 S8-3 기록 커밋(Markdown만), push·PR 전. 다음: S8-4 PR·리뷰·머지 → S8-5 1.0.0 공개(별도 명시 승인) → S8-6 공개 기록. 근거 이 PC의 `.superpowers/sdd/10-s7-s8-release/`(`s8-3-*`)와 Git common-dir `devbox-verification/s7/` |
| 2026-11-05–06 | Claude Code(Paseo) 컨트롤러·하위 에이전트 | S8-4 마무리(PR #653 독립 검토 수정 `7ea8451e`, CI·영수증, squash `08dffcd3`, #652 닫음, 정리)와 S8-5 보류(사용자 결정). 사용자 지시로 최종 교차 검토(main `08dffcd3`, 검토자 5명, 52건 → Critical 0·Important 21·Minor 31)와 사용자 결정("11건+문서 고친 뒤 공개 (권장)", CA-01 "자동 시작 유지, 문서만", 계획 승인 "한글 제목 변경(9)은 빼고 진행"). S8-4b-0–10(9 제외): 계획 `dfb4a2d0`, 코드 과제 1–8·10을 과제마다 구현 → 별도 검토 → 수정·범위 재검토(7은 수정 없음)로 `c8c0aca4`·`238d4cc7`·`a24f9cfc`·`564d0659`·`1ac7d85f`·`61930177`·`b118262d`·`ad793a54`·`70310423`. S8-4b-11 문서 `1752b478`, S8-4b-12 전체 검토(Ready, Minor 7)·시험 보강 `5d2d6f29`·`pnpm check`(clippy FAIL → 수정 `bc760aee`)·기록. 디스크 정리(공유 target 133G → 20G, CONVENTIONS §5 절차 밖, §1) | `fix/v1-final-review` HEAD = S8-4b-12 기록 커밋(이 커밋), push·PR 전. 다음: S8-4b-13 push·`windows.yml`·PR·§8 리뷰·squash. 근거 Git common-dir `devbox-verification/s8/`(`final-review/`, `sdd-10-s7-s8-release/`) |
