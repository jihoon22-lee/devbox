# S7 rc.2 보강 · S8 1.0.0 출시 실행 계획

- 작성: 2026-10-27. 기준 `origin/main@b6e5eba881c5fcb12eb229372138ff00e3003793`(v1.0.0-rc.1 공개·정리 인계 뒤).
- 승인 상태: 2026-10-27 사용자 결정 1–6(§1)으로 rc.2 방향을 승인했고, 같은 날 §2.2·§2.3에 "네 권장대로 진행해주세요"로 답해 **실행을 승인했다**(O1–O6 포함, O7 1.0 이후, D1 삭제, D2 켜기). 계획 작성은 제품 수정이나 성공 기준 달성을 뜻하지 않는다.
- 작업 위치
  - S7: `v1/s7-rc2`, `/home/jihoon/projects/devbox-wt/v1-s7-rc2`. origin/main에서 갈라진다.
  - S8: `v1/s8-release`, `/home/jihoon/projects/devbox-wt/v1-s8-release`. S7 머지 뒤 origin/main에서 갈라진다.
  - S8-4b: `fix/v1-final-review`, `/home/jihoon/projects/devbox-wt/fix-v1-final-review`. S8-4 머지 뒤 origin/main `08dffcd3`에서 갈라진다(사용자 결정 2026-11-06).
- 실행 방식: 하네스의 과제별 구현 → 다른 에이전트 검토(CONVENTIONS §11). 별도 스킬이나 특정 모델을 요구하지 않는다. 하위 에이전트가 없는 하네스는 같은 순서(구현 → 새 맥락에서 검토 → 커밋)를 직접 지킨다.
- 진행 원장: [PROGRESS](PROGRESS.md). 이 문서의 체크박스는 보조다.
- 입력
  - 2026-10-27 출시 전 검토(B1–B7, 권장 항목, UX·문서 소견)와 문서 감사.
  - 이 PC 설치본으로 확인한 계약: opencode 1.19.3, pi 1.3.0, Codex 0.164.0, Claude Code 2.1.308, tmux 3.6.

## 1. 사용자 결정 (2026-10-27)

| # | 결정(원문) | 이 계획에서 |
|---|---|---|
| 1 | "rc.2 계획 승인" | §2.1 범위. opencode·pi 기본 지원은 01 §2의 설계 범위 변경으로 기록한다 |
| 2 | "I1 를 1.0.0 출시 전에 고치는 게 나으면 고치자" | 고친다(S7-10). 실제 Windows로 확인하지 못하면 1.0.0 공개를 막는 NOT_RUN으로 남긴다 |
| 3 | "라이선스는 그냥 MIT로 해도 될 것 같아" | S7-12 |
| 4 | "문서 정리 꼭 하자" | 공개 설치 파일과 함께 나가는 문서는 S7-11, 통폐합·PROGRESS 재구성·계획 문서 보관은 S8-2 |
| 5 | "자동 리뷰가 켜져 있으면 이용하고(최대 3회) 없으면 1회의 독립 검토만" | CONVENTIONS §8 개정(S7-0), S7-15·S8-4에 적용 |
| 6 | "빌드 캐시는 삭제하자. sweep은 권장대로" | S7-0 |

## 2. 범위

### 2.1 포함 — 승인 항목과 그 의존

| ID | 내용 | 과제 |
|---|---|---|
| B1 | 실행 파일이 바뀌면 새 Claude 작업이 모두 `existing file differs`로 실패한다. 이전 세션의 hook·notify는 버전 불일치로 버려진다 | S7-1 |
| B2 | 설정 한 곳의 오류나 tmux 목록 실패로 데몬이 뜨지 않고, 앱에 복구할 길이 없다 | S7-2, S7-3 |
| B3 | 승인 대기 중 보내기가 붙여넣기+Enter로 승인을 눌러 버릴 수 있다(화면과 MCP 모두) | S7-4 |
| B4 | opencode·pi 미지원, Codex 새 worktree의 신뢰 화면 미감지 | S7-5 – S7-9 |
| B5 | 버전을 `1.0.0`으로 되돌림 | S8-1 |
| B6 | I1(Windows 사본 reader가 살아 있는 고아 자손을 놓침) | S7-10 |
| B7 | 실제 설치본 수용 공백: RC 설치, 업그레이드, 하네스 4종 SC3/SC4, 13개 확인, stable 공개 경로 | S7-14, S7-16, S8-5 |
| 의존 | Claude `PostToolUse`: 없으면 B3 차단이 승인 뒤에도 풀리지 않는다 | S7-4 |
| 의존 | CLI 경로 찾기·tmux 환경 갱신: pi가 로그인 PATH에 없다 | S7-6 |
| 의존 | 환경 점검·설치 안내 4종, 잘못된 hook 경고 제거 | S7-6 |
| 의존 | 고치는 화면의 문구 결함: 연결 화면, `messages.ts` 조사 | S7-3 |
| 문서 | P0 문서(README·설정 참조·SECURITY·낡은 줄), MIT·제3자 고지, 리뷰 규칙, 통폐합 | S7-0, S7-11, S7-12, S8-2 |

### 2.2 선택 — 사용자가 고른 것만 실행한다. 답이 없으면 1.0 이후로 둔다

| ID | 내용 | 권장 | 근거·크기 |
|---|---|---|---|
| O1 | 데몬 재시작 빠른 길(실행 파일·unit·환경이 같으면 재시작 생략) + N1 Windows 실행 브리지 재연결 | 포함 | 지금은 앱을 열 때마다 데몬을 재시작한다. hook 유실 창(최대 3초), N1(실행 중 Windows 작업의 준비 신호 유실), SC2 cold 9초의 원인이다. S6-5의 "항상 재시작" 결정을 뒤집는다. 크기 중 |
| O2 | Codex rollout 한도(파일 8 MiB, 한 줄 128 KiB)를 꼬리 읽기·긴 줄 건너뛰기로 바꿈 | 포함 | 긴 Codex 세션은 턴 종료 감지와 재개가 영구히 실패한다. 크기 소~중 |
| O3 | 환경 편집기(EnvEditor)의 늦은 저장이 다른 환경 초안을 덮음 | 포함 | S6 이전부터 있는 사용자 입력 섞임. 크기 소 |
| O4 | 준비 중 세션을 잃으면 소유 자원이 Partial로 강등돼 정리가 막힘 | 포함 | worktree·branch를 손으로 정리해야 한다. 크기 소 |
| O5 | 폐기 스냅샷이 무시 파일까지 해시함(한도 8 GiB, 100만 항목, 120초) | 포함 | `target/`·`node_modules`가 큰 worktree에서 폐기가 실패하거나 잠금을 오래 쥔다. 크기 중 |
| O6 | 새 작업 창과 에이전트 머리 보강: 프로젝트별 마지막 도구 기억, 브랜치 이름 칸(한글 제목은 지금 `task-<초>`), 첫 실행 [나중에] 기억, 원문 정리 오류를 정해진 문구로(SC5) | 포함 | 매일 쓰는 흐름의 마찰, SC5 부분 미달. 크기 소 |
| O7 | 실행기 패널의 OAuth 승인 연결 | 1.0 이후 | 기능 공백(대기 뒤 시간 초과). 손실은 없다 |
| — | 승인/거절 버튼, 범용 팔레트, 배지, 외부 편집기 선택 통합, `runtime.undefine` 화면, 프로필 편집 화면, MCP 자동 연결, 다중 하네스 비교·교차 검토, 오래된 `bin/devbox-<sha>` 정리, SC7 CI 시간 | 1.0 이후 | 새 기능·운영 개선 |

**사용자 답(2026-10-27, "네 권장대로 진행해주세요"):** O1–O6은 포함하고(S7-O에서 실행) O7은 1.0 이후로 둔다.

### 2.3 따로 답할 것

| ID | 질문 | 권장 |
|---|---|---|
| D1 | `~/.cache/targets/devbox-platform-check`(23 GiB)와 `devbox-native-lsp`(470 MiB)를 지울지. 둘 다 2026-09 v0.9 시기 cache다 | 삭제 |
| D2 | GitHub 비공개 취약점 신고(Private vulnerability reporting)를 켤지. SECURITY.md의 신고 경로가 된다. 저장소 설정 변경이라 승인이 필요하다 | 켜기 |

**사용자 답(2026-10-27, "네 권장대로 진행해주세요"):** D1 삭제(S7-0에서 완료), D2 켜기(이미 켜져 있어 변경 없음).

## 3. 공통 규칙

모든 과제의 요구 사항에 이 절이 포함된다.

**환경**
- WSL에서 `source ~/.cargo/env`, Node 24, pnpm 9, Rust 1.98.1을 쓴다.
- 자원 제한: cargo jobs 2, test threads 2, Vitest workers 2, CPU 4, nice +10, cgroup memory high/max 6/8 GiB, swap 1 GiB.
- 무거운 검증은 `python3 scripts/verify-resources.py --wait -- <명령>`으로 돌린다. Git common-dir 잠금이 경합하면 exit 75, 대기는 최대 60분이다.
- 시험 명령 표기: Rust는 `cargo test -p <crate> [--test <file>] <filter> -- --test-threads=2`, 화면은 `pnpm --filter app exec vitest run <path>`. 둘 다 위 wrapper 안에서 돌린다.

**과제와 커밋**
- 과제마다 순서: 좁은 실패 시험 확인 → 구현 → 그 시험과 직접 영향 시험 통과 → 다른 에이전트 검토 → 영어 Conventional Commit 1개.
  - 검토 기준은 00 §3.4다(S7-0에서 CONVENTIONS로 옮긴다). 이 문서의 시험·문구·경계를 지켰는지도 본다.
- 커밋은 `git -c user.name=jihoon22-lee -c user.email=188744452+jihoon22-lee@users.noreply.github.com commit`으로 하고, 하네스가 지정하는 공동 작성자 줄을 붙인다.
- 전체 `pnpm check`(clippy·build·E2E 포함)는 S마다 끝에서 한 번만 돌린다(S7-13, S8-3).
- 완료 커밋과 push 전에는 `pnpm exec biome ci .`를 돌린다.

**크기와 타입**
- `PROTOCOL_VERSION`은 S7·S8 동안 1로 둔다. S7-1의 Hello 완화는 이 값이 같다는 것을 전제로 한다. 바꿔야 하면 멈추고 사용자에게 묻는다.
- S7-12 뒤 고지의 입력이 바뀌는 커밋은 같은 커밋에서 `cargo run -p xtask -- notices`로 고지를 다시 만든다. 입력은 lock 파일 셋(`Cargo.lock`, `app/src-tauri/Cargo.lock`, `pnpm-lock.yaml`), manifest(작업 공간 root와 모든 멤버, 앱 crate, 앱 package), 함께 싣는 글꼴의 라이선스 본문, 링크되는 코드(Rust 표준 라이브러리와 std가 링크하는 crate, musl, WebView2 로더)의 라이선스 본문(`xtask/notices/linked/`, S8-2c 뒤), 설치 파일에 드는 코드(NSIS, `nsis_tauri_utils`)의 라이선스 본문(같은 폴더, S8-4 준비 뒤)이다(`xtask/src/notices/fingerprint.rs`). 버전 변경(S7-13, S8-1)도 여기에 해당한다.
- 파일 600줄, 컴포넌트 300줄 이하.
- `.github`+`scripts`는 1,982/2,000줄, workflow 2개다. 이 계획은 두 곳에 줄을 더하지 않는다. 생성기와 검사는 xtask와 기존 `cargo test --workspace`에 둔다.
- 경계 타입에 `Result<T,String>`과 `serde_json::Value`를 쓰지 않는다. 생성 파일은 `pnpm gen`으로만 바꾼다.

**문구**
- 화면 문구는 한국어이고 [ui-terms](../../../ui-terms.md)를 따른다. "데몬"은 진단·설정 화면에서만 쓴다.
- 사용자에게 보이는 오류는 정해진 코드와 "원인 + 다음 행동"으로 끝난다(SC5).

**하네스 CLI**
- 실제 실행은 계약 녹화(S7-5, S7-7, S7-8)와 Windows 수용(S7-14)에서만 한다.
- 단위·통합 시험은 녹화한 fixture로 실제 CLI 없이 통과해야 한다. CI에는 CLI가 없다.
- 녹화 때 만든 세션은 각 도구의 기록에 남는다. 사용자 자료이므로 지우지 않는다.
- 계약은 opencode ≥ 1.19.3, pi ≥ 1.3.0에서 확인한 것만 쓴다. 더 낮은 버전은 환경 점검이 경고한다.

**사용자 설정과 비밀**
- `~/.claude`, `~/.codex`, `~/.config/opencode`, `~/.pi`, `~/.bashrc`를 바꾸지 않는다. 하네스 연결은 작업 실행마다 주입한다.
- `~/.bashrc`의 자격 증명을 읽거나 출력하거나 복사하지 않는다.
- 환경 수집(`bash -lc env -0`)을 대화형 셸로 넓히지 않는다. 대화형 셸은 S7-6의 경로 찾기에서만 쓰며, 출력은 경로 한 줄만 쓰고 기록하지 않는다.

**공유 자원**
- 기존 서비스, prod 채널, tmux, systemd unit, Docker, 방화벽, 공유 네트워크를 시험으로 바꾸지 않는다.
- 소유 자원만 정리한다. 공유 cache, 사용자 자료, 활성·dirty·호스트 소유 worktree는 보존한다.

**Windows**
- 실기는 accept 채널 CI 산출물(`bash scripts/dogfood.sh <브랜치> --channel accept`)로만 한다.
- `windows-acceptance` 잠금을 잡고, PowerShell은 절대 경로 `/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe`로 부른다.

**보고**
- 실패하거나 실행하지 못한 Windows 실기·수동 항목은 PASS로 쓰지 않는다.
- 원본 FAIL·NOT_RUN과 근거 경로를 PROGRESS와 PR 본문에 남긴다.

**머지와 공개**
- 머지는 필수 CI 3개와 §8 리뷰 규칙을 마친 뒤 이 계획의 승인 범위에서 한다(00 §2.2와 같다).
- 태그 push와 GitHub Release는 rc.2와 1.0.0 각각 **그 시점의 별도 명시 승인** 뒤에만 한다.

## 4. 검토 초점

시험이 놓치기 쉬운 실제 사용 조건이다. 각 줄의 시험은 담당 과제에 있다.

1. **rc.1 세션이 살아 있는 채로 rc.2 설치.** 기존 세션의 hook이 계속 반영되고, 새 Claude 작업이 `existing file differs` 없이 시작된다. — S7-1 시험 + S7-14 U1
2. **`config.toml` 오타.** 데몬은 해당 기능만 끄고 뜬다. 화면이 파일·항목·다음 행동을 알려 주고, 파일 바이트는 그대로다. — S7-2, S7-3
3. **권한 승인 대기 중 화면·MCP에서 보내기.** 거절되고 pane에는 한 글자도 들어가지 않는다. — S7-4
4. **하네스 CLI가 데몬 PATH에 없거나, 로그인 셸과 대화형 셸이 다른 버전을 가리킴.** 환경 점검이 어느 도구인지와 고치는 법을 말한다. 작업 시작은 worktree를 만들기 전에 분명한 오류로 끝난다. — S7-6
5. **사용자 자신의 opencode 플러그인·pi 확장.** 그대로 로드되고, devbox 플러그인·확장은 devbox 작업 밖에서 아무것도 하지 않는다. — S7-7, S7-8 + S7-14 H3

## 5. 순서

| 묶음 | 과제 | 의존 | 커밋 |
|---|---|---|---|
| 착수 | S7-0 규칙·원장·cache | 계획 승인 | `docs: adopt the S7/S8 release plan and PR review rule` |
| S7-A | S7-1 B1 hook 안정 | S7-0 | `fix(agents): keep agent hooks working across binary updates` |
| S7-A | S7-2 B2 기능 저하 시작 | S7-0 | `fix(daemon): start in degraded mode on config and tmux errors` |
| S7-A | S7-3 B2 화면 | S7-2 | `fix(app): explain startup problems and connection failures` |
| S7-A | S7-4 B3 보내기 차단 | S7-1 | `fix(agents): refuse sending text while an agent waits for approval` |
| S7-B | S7-5 Codex 신뢰·hook 검토 화면 | — | `fix(agents): detect Codex folder trust and hook review prompts` |
| S7-B | S7-6 CLI 경로·환경 점검 4종 | S7-2 | `feat(agents): resolve agent CLIs like the user's shell and check all four` |
| S7-B | S7-7 opencode | S7-1, S7-6 | `feat(agents): support opencode as a builtin agent tool` |
| S7-B | S7-8 pi | S7-7 | `feat(agents): support pi as a builtin agent tool` |
| S7-B | S7-9 Shift+Enter·tmux 확장 키 | S7-8 | `feat(terminal): pass Shift+Enter through tmux extended keys` |
| S7-C | S7-10 I1 | — | `fix(runtime): keep Windows copy readers open while orphans run` |
| S7-C | S7-O1 – O6 중 승인된 것 | 각 과제 | 과제마다 하나 |
| S7-D | S7-11 문서 P0 | S7-1 – S7-10 | `docs: document rc.2 install, agent tools, MCP, config and security` |
| S7-D | S7-12 MIT·제3자 고지 | — | `chore: license under MIT and ship third-party notices` |
| S7-E | S7-13 브랜치 전체 검토·rc.2 버전·전체 검사 | 모든 제품 과제 | `chore(release): set version to 1.0.0-rc.2` |
| S7-E | S7-14 Windows 후보·accept 수용 | S7-13 | 결과 문서만 |
| S7-E | S7-15 PR·리뷰·머지 | S7-14 | — |
| S7-E | S7-16 rc.2 prerelease(별도 승인) | S7-15 | 태그만 |
| S8 | S8-0 – S8-6 | S7-16, 사용자 rc.2 확인 | §S8 |

묶음이 끝날 때마다 직접 영향 검사와 PROGRESS 묶음 행·현재 위치를 고쳐 커밋한다. push는 S7-14의 후보 직전에만 한다(00 §3.1).

---

## S7-0 착수 — 규칙·원장·cache

**목표:** 결정 5의 리뷰 규칙과 1.0 마무리 작업 규칙을 CONVENTIONS에 두고, 원장을 고치고, 결정 6의 cache를 정리한다. 제품 코드는 바꾸지 않는다.

**파일**
- 수정 `CONVENTIONS.md`
  - §8의 "자동 리뷰 상한" 문단을 아래 규칙으로 바꾼다.
  - 00 §3의 유효 규칙을 옮긴다: worktree·브랜치 이름, 인계 절차(§3.1), 먼저 물을 것(§3.2), 검토 기준(§3.4), 막혔을 때(§3.5), target 정리, accept 채널 수용, 최종 SHA 원칙(§3 :158).
  - 낡은 줄을 고친다:
    - :35 — 존재하지 않는 `.github/scripts/verify-resources.py`를 `scripts/verify-resources.py`로.
    - :24·:27·:34 — S 단위 예외를 일반 규칙으로.
    - :68 — #580이 무엇인지 설명.
- 수정 `AGENTS.md`
  - 포인터를 "규칙 CONVENTIONS · 현재 PROGRESS · 실행 10"으로 바꾼다.
  - 리뷰 줄을 결정 5에 맞춘다.
  - 하위 에이전트가 없는 하네스의 순서를 적는다.
- 수정 `00-roadmap.md`
  - §1 표에 이 문서를 추가한다.
  - §3 머리에 "유효 규칙은 2026-10-27 CONVENTIONS로 옮김" 한 줄을 둔다.
- 수정 `PROGRESS.md`
  - §1 현재 위치를 "S7 작업 중"으로 고친다.
  - §2 결정 표에 2026-10-27 행 6개를 넣는다.
  - 낡은 줄을 고친다: :21 "본 PR", :113 "모두 7개", :127 #650 머지 칸. 빠진 #639·#641·#643·#647 행을 추가한다.
- 수정 `.gitignore`: `.pi/tasks/` 추가(pi가 만든 저장소 루트의 작업 기록).

**§8 리뷰 규칙 원문** — 기존 "차단 기준" 문단은 유지한다.

```
PR 리뷰(사용자 결정 2026-10-27):
- 판정: PR을 연 뒤 10분 안에 `chatgpt-codex-connector[bot]`이 `codex-pull-request-review-summary` 댓글을 남기고, 그것이 사용량 한도 안내가 아니면 "자동 리뷰 켜짐"이다. 아니면 "꺼짐"이다. (#638–#645의 첫 반응은 PR 생성 0.1–0.3분 뒤, #646 이후는 반응 없음.)
- 켜짐: 리뷰가 끝난 서로 다른 head SHA 최대 3개까지 반영한다.
  - 지적은 코드·회귀 근거로 타당성을 판단해 고친다. 다음 push 전에 그 회차 수정을 모은다.
  - 4회째부터는 차단 기준만 고치고 나머지는 PROGRESS 후속 목록에 적는다.
  - 회차 리뷰가 push 뒤 60분 안에 끝나지 않거나 한도 안내가 오면 그때부터 꺼짐으로 본다.
- 꺼짐: 하네스의 독립 코드·보안 검토를 PR마다 한 번만 한다.
  - 타당한 지적은 고치고, 과제별 별도 검토·직접 영향 검사·필수 CI로 확인한다. PR 검토를 다시 하지 않는다.
  - 자동 리뷰는 NOT_RUN(꺼짐)으로 기록하고 PASS로 쓰지 않는다.
  - 켜짐 회차를 일부 쓴 뒤 꺼지면, 남은 회차 대신 독립 검토를 한 번 한다.
- PR 전 브랜치 전체 검토는 이 PR 리뷰와 별개로 한다.
- 회차·지적 수·판정·이월 항목을 PR 본문과 PROGRESS에 남긴다.
```

**단계**
- [ ] 1. 위 문서를 고친다. `git diff --stat`으로 문서 6개와 `.gitignore`만 바뀌었는지 확인한다.
- [ ] 2. 문서 검토 에이전트가 확인할 것:
  - 00 §3에서 옮긴 규칙과 원문의 뜻이 같은지.
  - 결정 5 원문과 §8 문단이 맞는지.
  - 깨진 상대 링크가 없는지.
- [ ] 3. cache 정리. 결정 6, "권장대로"는 00 §3 :187의 기준을 뜻한다.
  1. `pgrep -a -x cargo; pgrep -a -x rustc`가 비어 있을 때만 진행한다.
  2. target-dir은 `~/.cargo/config.toml`의 전역 `target-dir = "/home/jihoon/.cache/targets/devbox"`다. 다른 Rust 프로젝트도 함께 쓰므로 산출물만 지운다(다시 만들 수 있음).
  3. `~/.cache/targets/devbox-s5`(70 GiB, S5 전용)가 심볼릭 링크가 아닌지 확인한 뒤 `rm -rf --one-file-system`으로 지운다.
  4. `du -sh ~/.cache/targets/devbox/{debug,tmp,x86_64-pc-windows-msvc,x86_64-unknown-linux-musl} ~/.cache/targets/devbox/debug/incremental`을 기록한다.
  5. `cargo install cargo-sweep --locked`로 설치한다. 00 §3.3에 따라 묻지 않는 사용자 cargo 도구다. `cargo sweep --help`로 옵션을 확인한 뒤, 최근 3일 동안 쓰지 않은 산출물을 지운다.
  6. 다시 잰다. 150 GiB 아래가 아니면 `debug/incremental/`만 비운다. 의존성 산출물은 남겨 새 worktree의 첫 빌드를 줄인다.
  7. D1이 "삭제"면 두 cache를 같은 방식으로 지운다.
  8. `scripts/sweep.sh`는 고치지 않는다(정리 유보 no-op, 예산).
  9. 전후 크기를 PROGRESS §1에 한 줄로 남긴다.
- [ ] 4. `pnpm exec biome ci .` 뒤 커밋: `docs: adopt the S7/S8 release plan and PR review rule`.

## S7-1 B1 — 업데이트 뒤에도 hook이 끊기지 않게

**목표:** 세 가지를 고쳐 실행 파일이 바뀌어도 hook이 끊기지 않게 한다.
- hook 산출물은 내용 주소 파일로 쓴다.
- hook과 시작 스크립트는 안정 경로 `…/bin/devbox`를 쓴다.
- CLI·MCP 클라이언트는 프로토콜만 같으면 받는다.

**파일**
- 만들기 `crates/agents/src/artifacts.rs`
- 수정 `crates/agents/src/prepare.rs:145-165`(전역 `claude.json` 생성 대신 산출물 쓰기), `crates/agents/src/lib.rs`(모듈 등록)
- 만들기 `crates/cli/src/daemon/exe.rs`
- 수정 `crates/cli/src/daemon/mod.rs:94-98`(`devbox_bin`)
- 수정 `crates/cli/src/daemon/server.rs:233-264`(Hello 판정)
- 시험:
  - `crates/agents/src/artifacts.rs` 안 시험
  - `crates/cli/src/daemon/exe.rs` 안 시험
  - `crates/cli/src/daemon/server/tests.rs`
  - `crates/cli/tests/agents.rs`

**인터페이스 (Produces)**
- `pub(crate) fn write_artifact(dir: &Path, stem: &str, ext: &str, body: &str) -> std::io::Result<PathBuf>`
  - 파일 이름은 `<dir>/<stem>-<sha256(body) 앞 16 hex>.<ext>`, 권한은 0600 create_new다.
  - 같은 이름이 있으면, 심볼릭 링크가 아닌 일반 파일이고 바이트가 같을 때만 그 경로를 돌려준다. 아니면 `ErrorKind::AlreadyExists`다.
  - 기존 `hook_files` 잠금 안에서 부른다. S7-7·S7-8이 같은 함수로 플러그인과 확장을 쓴다.
- `pub(crate) fn stable_devbox_path(exe: &Path) -> PathBuf`
  - 파일 이름이 `devbox-<64 소문자 hex>`이고, 같은 폴더의 `devbox`가 심볼릭 링크이며, 둘의 canonical 경로가 같을 때만 그 링크 경로를 돌려준다.
  - 그 밖(개발 빌드, `pnpm dev`, 링크가 다른 파일을 가리킴)에는 `exe`를 그대로 돌려준다.
- `pub(crate) fn hello_accepts(client: ClientKind, version: &str, protocol: u32, daemon_version: &str) -> bool`
  - `protocol == PROTOCOL_VERSION`이 먼저 성립해야 한다.
  - 그다음 `version == daemon_version`이거나, `client`가 `Cli` 또는 `Mcp`일 때 true다.
  - `App`·`Dev`·`Test`는 지금처럼 정확히 같아야 한다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `artifacts::same_body_reuses_the_same_file`
  - `artifacts::different_body_gets_a_new_name_and_keeps_the_old_file`: 옛 파일의 바이트가 그대로다.
  - `artifacts::symlink_or_changed_file_is_rejected`
  - `exe::uses_the_symlink_when_it_points_to_current_exe`
  - `exe::falls_back_when_the_symlink_points_elsewhere`
  - `exe::falls_back_for_unhashed_names`: `target/debug/devbox`.
  - `server::tests::cli_and_mcp_hello_accept_another_version_with_the_same_protocol`
  - `server::tests::app_hello_rejects_another_version`
  - `server::tests::every_client_rejects_another_protocol`
  - `agent_event::rc1_shaped_agent_event_applies_in_the_new_daemon`(`crates/cli/tests/agent_event.rs`)
    - rc.1(`28eb2a91`)의 `AgentEventParams` 필드 집합 그대로인 JSON을 문자열로 고정한다: `agentId`, `launchId`, `source`, `payload`, `code`.
    - 다른 버전 문자열을 보낸 CLI 연결로 `agents.event`를 보낸다.
    - 작업 상태가 바뀌어야 한다. 연결만 받아들이고 요청이 역직렬화에서 실패하면 검토 초점 1을 지키지 못한다.
  - `agents::claude_task_starts_after_the_devbox_path_changes`
    - 같은 데이터 폴더에서 `devbox_bin`을 `/a/devbox`로 한 번, `/b/devbox`로 한 번 Claude 작업을 준비한다.
    - 두 번 다 성공해야 한다.
    - 두 설정 파일 경로가 서로 달라야 한다.
    - 각 파일 내용에 자기 경로가 들어 있어야 한다.
- [ ] 2. `cargo test -p devbox-agents artifacts`, `cargo test -p devbox-cli exe`, `cargo test -p devbox-cli server::tests`, `cargo test -p devbox-cli --test agents claude_task_starts_after`를 돌린다. 새 시험이 모두 실패해야 한다. 마지막 시험은 `existing file differs`로 실패한다.
- [ ] 3. 위 인터페이스대로 구현한다.
  - `devbox_bin = stable_devbox_path(&current_exe()?)`로 바꾼다. 이 값 하나로 hook 명령, Codex `notify`, 시작 스크립트 `DEVBOX_BIN`이 모두 안정 경로가 된다.
  - 옛 `agent-hooks/claude.json`은 지우지 않는다. 이미 실행 중인 세션이 쓰고 있을 수 있다.
- [ ] 4. 같은 시험을 다시 돌린다. 모두 PASS여야 한다. 기존 `cargo test -p devbox-agents`와 `cargo test -p devbox-cli --test agent_event`도 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `fix(agents): keep agent hooks working across binary updates`.

## S7-2 B2 — 설정·tmux 오류에도 데몬이 뜨게

**목표:** 설정 오류는 그 기능만 끄고 기록한다. tmux 목록 실패는 배경에서 다시 시도한다. 데몬은 어느 경우에도 뜬다.

**파일**
- 수정 `crates/core/src/config.rs:140-158`: 너그러운 읽기 추가.
- 수정 `crates/cli/src/daemon/mod.rs:60-100`: 너그러운 읽기 사용, 노트·검색·프로필 실패를 기록하고 끔.
- 수정 `crates/agents/src/profile.rs`
  - `waiting_patterns` 별칭.
  - 항목별 너그러운 프로필 읽기.
- 수정 `crates/cli/src/daemon/server.rs:68-72`: `agents.initialize()` 실패를 배경 재시도로.
- 수정 `system.info` 결과 타입(현재 정의 위치): `startupProblems` 추가. 그 뒤 `pnpm gen`.
- 시험:
  - `crates/core/src/config.rs` 시험 모듈
  - `crates/agents/src/profile.rs` 시험 모듈
  - `crates/cli/tests/daemon.rs`

**인터페이스 (Produces)**
- `pub fn load_lenient(paths: &Paths) -> ConfigLoad`
  - `pub struct ConfigLoad { pub config: Config, pub disabled: Disabled, pub problems: Vec<StartupProblem> }`
  - `Disabled { notes: bool, search: bool, windows: bool }`
- `StartupProblem`: 경계 타입, serde tag `kind`, camelCase.
  - `ConfigFile { file, message }`
  - `ConfigSection { file, section, message }`
  - `AgentsReconcile { message }`
- `pub fn configured_lenient(config: &AgentsConfig) -> (Vec<ToolProfile>, Vec<StartupProblem>)`
  - 잘못된 항목과 중복 id(두 번째 이후)만 건너뛴다.
  - 기본 프로필은 늘 남는다.

**영역별 정책.** 이 표가 기준이며 S7-11의 `docs/config.md`에 그대로 옮긴다.

| 영역 | 읽기 실패 시 |
|---|---|
| 파일 전체 문법 오류 | 아래 모든 영역을 실패로 처리 |
| `[notes]` | 노트를 끈다. 기본 vault로 대체하지 않는다 |
| `[search]` | 검색을 끈다 |
| `[windows]`(config.local.toml) | Windows 사본을 쓰는 실행을 `windows.config_invalid`로 시작하지 않는다 |
| `[agents]` | 닫은 작업의 자동 정리를 끈다(`cleanup_after_close = false`). 도구 프로필은 아래 줄대로 확인한다 |
| `[[agents.profiles]]` | 잘못된 항목·중복 id만 건너뛴다 |
| `[mcp]` | `write = false` |
| `[activity]` | 활동을 기록하지 않는다 |
| 그 밖의 영역 | 기본값 |

**설정 파일 쓰기:** `secrets`·`activity`의 toml_edit 쓰기는 원문 파싱이 실패하면 거절하고, 다른 영역의 바이트는 보존한다. 이 동작을 시험으로 고정한다(검토 초점 2).

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `config::syntax_error_yields_defaults_disables_user_paths_and_reports_the_file`: `disabled`가 셋 다 true, 문제 1건, `file`은 config.toml 경로.
  - `config::invalid_section_only_disables_that_section`: 잘못된 `[mcp]`면 `write=false`이고 `[projects]`는 읽힌다.
  - `config::local_file_problem_names_the_local_file`
  - `profile::snake_case_waiting_patterns_alias_is_accepted`
  - `profile::invalid_profile_is_skipped_and_reported_while_builtins_remain`
  - `profile::duplicate_custom_id_keeps_the_first_and_reports_the_second`
  - `daemon::daemon_starts_with_a_broken_config_and_reports_problems`
    - `config.toml`에 깨진 `[[agents.profiles]]`를 넣고 시험 데몬을 띄운다.
    - `system.ping`이 성공해야 한다.
    - `system.info.startupProblems`에 해당 문제가 있어야 한다.
    - 기본 프로필 목록이 그대로여야 한다.
  - `daemon::broken_config_bytes_are_unchanged_after_a_settings_write_attempt`
    - 문법 오류 파일에서 비밀 이름 갱신 RPC가 오류로 끝나야 한다.
    - 파일 SHA-256이 그대로여야 한다.
  - `daemon::valid_toml_with_an_invalid_section_survives_an_activity_rule_write`: 잘못된 `[[agents.profiles]]` 블록 바이트가 그대로여야 한다.
  - `daemon::agents_initialize_failure_does_not_stop_the_daemon`
    - 가짜 tmux가 목록 실패를 내게 한다.
    - 데몬이 응답해야 한다.
    - `AgentsReconcile` 문제가 있어야 한다.
    - tmux가 정상으로 돌아오면 문제가 사라져야 한다.
- [ ] 2. 위 시험을 돌린다. 모두 실패해야 한다(데몬 시작 실패 또는 함수 없음).
- [ ] 3. 구현한다.
  - 설정 파일마다 먼저 `toml::Table`로 읽는다. 알려진 영역을 영역별로 따로 역직렬화한다.
  - `initialize` 재시도 간격은 1초, 2초, 5초, 10초, 그 뒤 30초마다다. 성공하면 문제를 지운다.
  - 노트·검색을 끈 동안 해당 RPC는 정해진 오류 코드로 답한다. 기존 "vault 미설정" 상태가 있으면 그것을 쓴다.
- [ ] 4. 같은 시험과 `cargo test -p devbox-core`, `cargo test -p devbox-cli --test daemon`을 돌린다. 모두 PASS여야 한다.
- [ ] 5. `pnpm gen` 뒤 생성 파일 변화를 확인한다. 검토 뒤 커밋: `fix(daemon): start in degraded mode on config and tmux errors`.

## S7-3 B2 화면 — 시작 문제 안내와 연결 화면 복구

**목표:**
- 기능이 꺼진 이유와 고치는 법을 화면에 보인다.
- 첫 연결 전 실패에서도 원인을 볼 수 있게 한다.
- 고치는 화면의 문구 결함을 함께 바로잡는다.

**파일**
- 만들기 `app/src/shell/StartupProblems.tsx`(150줄 이하)와 시험 파일. 주 화면 머리에 둔다.
- 수정 `app/src/shell/ConnectionGate.tsx:55-58, :108, :190-207`
- 수정 `app/src-tauri/src/platform/supervisor.rs`: 로그 꼬리 명령.
- 수정 `app/src/rpc/messages.ts:502, :505`

**인터페이스**
- Tauri 명령 `daemon_log_tail(lines: u16) -> Result<String, AppError>`
  - `wsl.exe -d <배포판> -- journalctl --user -u <daemon unit>@<instance>.service -n <lines> --no-pager -o cat`를 실행한다.
  - `lines`는 최대 200, 출력은 64 KiB에서 자른다.
- 화면은 `system.info.startupProblems`를 읽는다. 문제가 있는 동안 10초마다 다시 읽는다.

**문구 (정확히)**

| 경우 | 제목 | 본문 |
|---|---|---|
| ConfigFile | 설정 파일을 읽지 못했습니다 | `{file}`에 문법 오류가 있어 기본값으로 실행 중입니다. 노트·검색·Windows 사본과 사용자 정의 도구 프로필이 꺼져 있습니다. 파일을 고친 뒤 [다시 설정]을 누르세요. |
| ConfigSection | 설정 일부를 읽지 못했습니다 | `{file}`의 [{section}] 항목에 오류가 있어 이 기능을 껐습니다. 고친 뒤 [다시 설정]을 누르세요. |
| AgentsReconcile | 에이전트 상태를 확인하지 못했습니다 | 터미널 서버(tmux)에 연결하지 못했습니다. 자동으로 다시 시도합니다. 계속되면 설정 → 앱 진단을 확인하세요. |
| 연결 화면 :55-58 | WSL 쪽 devbox를 준비하고 있습니다 | 내장 실행 파일을 확인하고 필요한 서비스를 시작합니다. |
| 연결 화면 :108 | WSL 쪽 devbox에 연결하지 못했습니다 | [자세히]에서 원인을 확인하고 [다시 시도]하세요. |

- 원문 `message`는 세부 줄로만 보인다.
- [다시 설정]은 앱 진단의 같은 버튼과 같은 동작을 한다. [경로 복사]를 함께 둔다.
- 연결 화면의 [자세히]는 실패한 setup 단계와 로그 꼬리를 보여 주고 [복사]를 둔다.
- "에이전트 작업의 터미널 세션은 그대로 유지됩니다"는 지운다. Windows SC4는 아직 NOT_RUN이다.
- `messages.ts`: "프로젝트 설정 파일를" → "프로젝트 설정 파일을", "프로젝트 설정 파일가" → "프로젝트 설정 파일이".

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `StartupProblems.test.tsx`:
    - 세 경우의 제목·본문·세부가 보인다.
    - 문제가 없으면 아무것도 그리지 않는다.
    - [다시 설정]이 재설정 명령을 한 번 부른다.
  - `ConnectionGate.test.tsx`:
    - 실패 상태에서 [자세히]가 `daemon_log_tail` 결과를 보이고 [복사]가 있다.
    - 첫 연결 전 문구 어디에도 `/데몬/`이 없다.
  - `messages.test.ts`: 모든 메시지에 `/파일를|파일가/`가 없다.
  - `supervisor`: `log_tail_command_targets_the_instance_unit_and_caps_output`.
- [ ] 2. 위 시험이 실패해야 한다.
- [ ] 3. 구현한다. 컴포넌트는 300줄 이하로 유지한다.
- [ ] 4. Vitest 관련 파일과 `cargo test --manifest-path app/src-tauri/Cargo.toml --lib platform::supervisor`를 돌린다. 모두 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `fix(app): explain startup problems and connection failures`.

## S7-4 B3 — 승인 대기 중 보내기 차단

**목표:**
- 작업 상태가 `Waiting`이면 데몬이 보내기를 거절한다. 화면과 MCP가 같은 경로를 쓴다.
- Claude가 승인 뒤 다시 일하면 `Waiting`을 푼다.

**파일**
- 수정 `crates/agents/src/send.rs:23-58`: 붙여넣기 직전 상태 검사.
- 수정 `crates/agents/src/hooks.rs`
  - `claude_settings`에 `PostToolUse`를 추가한다.
  - `parse`에서 `PostToolUse`를 `AgentEvent::OutputResumed`로 매핑한다. `Waiting → Running`이며, 다른 상태에서는 변화가 없다.
- 오류 코드 `agents.awaiting_input`을 추가하고 `app/src/rpc/messages.ts`에 문구를 둔다.
- 수정 `app/src/features/agents/detail/AgentDetail.tsx:95-96`, `Compose.tsx`
- 시험:
  - `crates/cli/tests/agents.rs`
  - `crates/cli/tests/mcp.rs`
  - `crates/agents/src/hooks.rs` 시험 모듈
  - `Compose.test.tsx`

**문구**
- 오류 제목: "에이전트가 선택을 기다리고 있습니다."
- 다음 행동: "터미널에서 승인이나 선택을 마친 뒤 다시 보내세요."
- 입력 칸 안내: "에이전트가 터미널에서 선택을 기다리는 동안에는 보낼 수 없습니다." 옆에 [터미널 보기]를 둔다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `agents::send_is_refused_while_waiting_and_the_pane_receives_nothing`
    - `NeedsInput` 이벤트로 `Waiting`을 만든다.
    - `agents.send`가 `agents.awaiting_input`으로 끝나야 한다.
    - pane 캡처와 tmux 버퍼에 보낸 글자가 없어야 한다.
    - `OutputResumed` 뒤에는 같은 보내기가 성공해야 한다.
  - `mcp::agent_send_reports_awaiting_input`: `[mcp] write = true`.
  - `hooks::claude_settings_include_post_tool_use`
  - `hooks::post_tool_use_maps_to_output_resumed`
  - `Compose.test.tsx`
    - 대기 상태에서는 보내기 버튼이 비활성이고, Ctrl+Enter로도 호출이 없고, 안내가 보인다.
    - Idle·Running 상태에서는 보낼 수 있다.
- [ ] 2. 시험이 실패해야 한다. 지금은 붙여넣기가 일어난다.
- [ ] 3. 구현한다.
  - 상태 검사는 같은 작업의 붙여넣기 직전에 한다.
  - MCP는 같은 서비스 함수를 부르므로 따로 막지 않는다.
  - hook 목록이 바뀌면 S7-1의 산출물 이름도 바뀐다. 이것은 정상이다.
- [ ] 4. 같은 시험과 `cargo test -p devbox-agents state`를 돌린다. 모두 PASS여야 한다.
- [ ] 5. 실제 Claude 승인 화면에서 붙여넣기+Enter가 어떻게 동작하는지는 S7-14 P1에서 기록한다. 그 결과는 문구만 보정하며, 차단은 결과와 상관없이 유지한다.
- [ ] 6. 검토 뒤 커밋: `fix(agents): refuse sending text while an agent waits for approval`.

## S7-5 Codex — 폴더 신뢰·hook 검토 화면 감지

**목표:** 새 worktree에서 Codex가 묻는 폴더 신뢰 화면과 "Hooks need review" 화면을 대기로 잡는다.

**파일**
- 만들기 `crates/agents/tests/fixtures/codex-0.164.0/{trust,hooks-review,working}.txt`
- 수정 `crates/agents/src/profile.rs`: builtin codex `waiting_patterns`에 두 정규식을 추가한다.
- 시험 `crates/agents/src/watch_tests.rs`

**fixture 녹화** — 실제 `~/.local/bin/codex` 0.164.0, 소유 자원만 쓴다.
1. `/tmp/devbox-codex-fixture-<난수>`에 git 저장소를 만든다.
2. tmux 전용 socket(`tmux -L devbox-fixture-<난수>`)에서 200×50 세션으로 `codex`를 띄운다. 5초 뒤 `capture-pane -p -J`로 신뢰 화면을 얻는다.
3. 아무것도 고르지 않고 Ctrl+C로 끝낸 뒤 server를 끄고 폴더를 지운다.
4. 시작 전후 `~/.codex/config.toml`의 SHA-256이 같아야 한다. 바뀌었으면 멈추고 사용자에게 알린다. 손으로 되돌리지 않는다.
5. 홈 경로는 `/home/user`로 바꿔 저장한다.
6. hook 검토 화면을 사용자 설정을 바꾸지 않고 띄울 수 없으면, 실행 파일 문자열(`strings`)에서 화면 문장을 가져와 "문자열 기반"이라고 표시한다. 실제 화면 확인은 S7-14에서 나타날 때만 하고, 나타나지 않으면 NOT_RUN이다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `codex_trust_screen_is_waiting`
  - `codex_hooks_review_screen_is_waiting`
  - `codex_working_screen_is_not_waiting`
  - 기존 승인 화면 시험은 그대로 둔다.
- [ ] 2. `cargo test -p devbox-agents --lib watch::tests`: 새 시험 2개가 실패해야 한다.
- [ ] 3. 패턴을 추가한다. 화면의 안정적인 문장에 고정하고, 대소문자를 무시하는 여러 줄 정규식으로 쓴다. 근거로 fixture 경로를 주석에 적는다.
- [ ] 4. 같은 시험이 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `fix(agents): detect Codex folder trust and hook review prompts`.

## S7-6 CLI 경로·환경 점검 4종·tmux 환경 갱신

**목표:**
- 작업은 사용자가 터미널에서 쓰는 그 CLI로 실행한다.
- 없거나 낡았으면 시작 전에 분명히 실패하고, 환경 점검이 고치는 법을 말한다.

**파일**
- 만들기 `crates/agents/src/command_path.rs`
- 수정 `crates/agents/src/prepare.rs`: 자원을 만들기 전에 해석한다.
- 수정 `crates/agents/src/start.rs`: 해석한 폴더를 PATH에 넣는다. 공유 폴더(`/usr/bin` 등)면 PATH 끝에 붙이고 절대 경로로 실행하며, 그 밖의 폴더는 앞에 둔다(S7-11 정정, Ruling 23·26).
- 수정 `crates/tools/src/doctor/probe.rs:41-49`: `AgentTool { Claude, Codex, Opencode, Pi }`.
- 수정 `crates/tools/src/doctor/checks.rs:110`: 목록에 `opencode`, `pi`를 추가한다.
- 수정 `crates/tools/src/doctor/extended.rs:29-35, :114-139`: 전역 hook 경고를 지운다.
- 수정 `crates/tools/src/install.rs:47-48`
- 수정 `crates/terminal/src/tmux.rs`, `crates/cli/src/daemon/system.rs:44-52`
- 오류 코드 `agents.tool_missing`을 추가하고 `messages.ts`에 문구를 둔다.

**인터페이스 (Produces)**
- `pub enum CommandSource { InteractiveShell, ServicePath }`
- `pub struct ResolvedCommand { pub program: PathBuf, pub source: CommandSource, pub version: Option<String> }`
- `pub async fn resolve(program: &str, service_path: &OsStr, probe: &impl ShellProbe) -> Option<ResolvedCommand>`
  - `program`에 `/`가 있으면 그 파일만 확인한다.
  - 그 밖에는 로그인 대화형 셸(Devbox 터미널의 `bash -l`과 같은 시작 파일을 읽는다)을 먼저 찾고, 없으면 서비스 PATH(설정 env 파일)를 찾는다. 사용자가 터미널에서 쓰는 것과 같은 실행 파일을 쓰기 위해서다. 계획을 쓸 때 이 PC의 Codex는 로그인 0.146.0, 대화형 0.164.0이어서 대화형 셸(`bash -ic`)로 충분했다. 2026-10-28·10-30 standalone 설치 뒤에는 `~/.profile`이 `~/.bashrc` 다음에 `~/.local/bin`을 앞에 붙여, 대화형 셸만으로는 사용자 터미널과 다른 0.146.0을 찾았다. 그래서 S7-14에서 로그인 대화형 셸로 고쳤다(P-DEF-1, `40d97a83`).
  - 셸이 답한 결과만 5분 동안 cache한다. 셸이 답하지 않으면 그 호출에만 서비스 PATH 결과를 쓰고 다음 호출에서 셸에 다시 묻는다. 환경 점검은 cache를 읽지 않고 매번 셸을 실행한다(S7-11 정정, Ruling 21·22).
- `pub trait ShellProbe`. 실제 구현은 아래 명령 하나를 실행한다.
  - 명령: `bash -lic 'printf "\036%s\036" "$(command -v -- "$1")"' devbox-probe <program>`. 조회 자식 환경에만 `MOTD_SHOWN=devbox-probe`를 둬 로그인 셸이 그날의 MOTD를 소모하지 않게 한다(S7-14 P-DEF-1).
  - stdin은 `/dev/null`, stderr는 버린다. 5초 timeout, stdout 64 KiB cap.
  - 마지막 `\x1e` 쌍 사이 값이 절대 경로이고 실행 가능할 때만 쓴다.
  - stdout 원문은 기록하지 않는다.
- `Tmux::set_global_env(&self, key: &str, value: Option<&str>) -> Result<(), TerminalError>`

**동작**
- 시작 스크립트는 해석한 실행 파일의 폴더를 `export PATH=<폴더>:"$PATH"`로 앞에 둔다. `node` 같은 shebang 도구도 같은 폴더에서 찾게 하기 위해서다. 공유 폴더면 `export PATH="$PATH":<폴더>`로 끝에 붙이고 절대 경로로 실행한다(S7-11 정정).
- 해석하지 못하면 worktree·branch를 만들기 전에 `agents.tool_missing { command }`로 끝난다.
  - 제목: "{command} 명령을 찾을 수 없습니다."
  - 다음 행동: "설정 → 앱 진단 → 환경 점검에서 설치 방법을 확인하세요."
- 환경 점검은 도구마다 경로·출처·버전을 보인다.
  - 출처가 서로 다르면 Ok와 함께 안내한다: "로그인 셸과 대화형 셸이 다른 {tool} 실행 파일을 가리킵니다. devbox는 대화형 셸의 {path}({version})를 씁니다."
  - opencode < 1.19.3, pi < 1.3.0이면 경고한다.
  - 전역 hook 경고는 Ok "devbox가 작업마다 연결합니다"로 바꾼다.
- 설치 안내
  - opencode: `npm i -g opencode-ai`. 구현 때 `npm view opencode-ai name version`으로 공식 패키지인지 확인한다. 아니면 공식 설치 안내 문장만 둔다.
  - pi: `npm i -g @earendil-works/pi-coding-agent`.
- `refresh_env`는 env 파일을 바꾼 뒤 허용 목록 key의 변화만 tmux 전역 환경에 반영한다. 바뀐 key는 set, 사라진 key는 `-gu`. 실패는 기록만 하고 refresh는 성공이다. 이미 떠 있는 pane은 바뀌지 않는다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `command_path::prefers_the_interactive_shell_and_reports_both_sources`
  - `command_path::falls_back_to_the_service_path`
  - `command_path::reads_only_the_value_between_markers`: 앞뒤 잡음 출력 포함.
  - `command_path::a_late_shell_is_abandoned_after_five_seconds`(계획 이름 `timeout_or_relative_result_returns_none`에서 바뀜. 상대 경로 경우는 `reads_only_the_value_between_markers`의 `answer()` 시험으로 옮김, S7-11 정정)
  - `start::resolved_tool_directory_is_prepended_to_path`, 공유 폴더는 `start::a_cli_in_a_shared_folder_runs_by_its_path_with_the_folder_last`
  - `agents::start_with_a_missing_tool_fails_before_creating_a_worktree`: 오류 코드를 확인하고, branch와 worktree가 없어야 한다.
  - `checks_tests::reports_all_four_agent_tools_with_versions`
  - `checks_tests::low_opencode_or_pi_version_warns`
  - `extension_tests::global_hook_warning_is_gone`
  - `tmux::set_global_env_reaches_new_sessions_only`: 소유 socket에서 실제 tmux로.
  - `system::refresh_env_updates_changed_allowlisted_keys_in_tmux`: 가짜 runner로.
- [ ] 2. 시험이 실패해야 한다.
- [ ] 3. 구현한다. 셸 문법이 섞인 프로필 명령은 첫 단어가 단순 이름일 때만 해석하고, 아니면 지금처럼 그대로 실행한다.
- [ ] 4. `cargo test -p devbox-agents command_path start`, `cargo test -p devbox-tools doctor`, `cargo test -p devbox-terminal tmux`, `cargo test -p devbox-cli --test agents start_with_a_missing_tool`을 돌린다. 모두 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `feat(agents): resolve agent CLIs like the user's shell and check all four`.

## S7-7 opencode 기본 지원

**계약** — 2026-10-27 opencode 1.19.3. 구현 시작 때 다시 확인하고 fixture로 남긴다.
- 연결 주입: `OPENCODE_CONFIG_CONTENT='{"plugin":["file://<절대 경로>"]}'`. 사용자 설정과 병합되어 사용자 플러그인이 유지된다(`opencode debug config`로 확인함).
- 시작은 `opencode --prompt=<text>`, 재개는 `opencode -s <sessionID>`다. `--pure`는 쓰지 않는다. 플러그인이 꺼진다.
- 플러그인 이벤트(v1): `session.status`(`properties.status.type`: busy·retry·idle, `properties.sessionID`), `permission.asked`, `permission.replied`, `session.created`·`session.updated`(`properties.info.parentID`).
- 사용량: `opencode export <sessionID>`의 JSON.
- 플러그인 export 형태는 설치된 1.19.3이 읽는 형태로 정한다. 공식 문서와 사용자 플러그인 파일을 읽기만 해서 확인한다.

**파일**
- 수정 `crates/agents/src/profile.rs`
  - `Hooks::{Claude, Codex, Opencode, Pi, None}`. 기존 값 `claude`·`codex`·`none`은 그대로 읽힌다.
  - builtin 순서는 claude, codex, opencode, pi. 이 과제에서 opencode를 추가한다.
- 수정 `crates/agents/src/hooks.rs`: `HookSource::{…, Opencode, Pi}`와 `FromStr` `"opencode"`·`"pi"`.
- 수정 `crates/cli/src/main.rs:118`: `value_parser`에 `"opencode"`, `"pi"` 추가.
- 만들기 `crates/agents/src/plugin_event.rs`
- 만들기 `crates/agents/assets/opencode-plugin.ts`
  - opencode는 Bun으로 TS 플러그인을 바로 읽는다. 구현 시작 때 1.19.3이 `file://…/x.ts`를 읽는지 확인한다.
  - 읽지 못하면 `.js`와 같은 폴더의 `.d.ts`로 바꾼다.
  - 자산은 앱 시험이 import하므로 앱 typecheck에 들어간다. Node 타입(`node:child_process`, `process`)이 앱 tsconfig에서 풀리는지 확인하고, 안 풀리면 자산 안에 최소 선언을 둔다.
- 수정 `biome.json` includes: `crates/agents/assets/**/*.ts`.
- 수정 `crates/agents/src/start.rs`
  - `StartPlan.claude_settings`를 `hook_file`로 이름을 바꾼다. Claude 설정, opencode 플러그인, pi 확장이 함께 쓴다.
  - opencode 분기를 추가한다.
- 수정 `crates/agents/src/prepare.rs`(플러그인을 `write_artifact`로), `crates/agents/src/resume.rs:84-133`
- 만들기 `crates/agents/src/usage_plugins.rs`, 수정 `crates/agents/src/usage.rs:316-372`
- 수정 `app/src/features/agents/model.ts:29-34`: `hookMissing`을 `hooks !== "none"`인 모든 프로필에 적용.
- fixture `crates/agents/tests/fixtures/opencode-1.19.3/{events.jsonl,export.json}`
- 시험 `app/src/features/agents/harness/opencodePlugin.test.ts`(Vitest, 자산을 상대 경로로 import)

**인터페이스 (Produces)** — S7-8이 같은 것을 쓴다.
- 플러그인 → 데몬 payload. 마지막 argv 하나로 넘긴다(Codex와 같음).
  - 형태: `{"v":1,"event":"working"|"idle"|"needsInput"|"inputResolved","sessionId"?:string,"message"?:string}`
  - Rust 타입 `PluginEvent`: camelCase, `deny_unknown_fields`.
- `pub fn parse_plugin_event(source: HookSource, payload: &str) -> Vec<AgentEvent>`
  - `sessionId`가 처음 보이면 앞에 `SessionStarted`를 붙인다.
  - `working` → `PromptSubmitted`, `idle` → `TurnComplete { message }`, `needsInput` → `NeedsInput { message }`, `inputResolved` → `OutputResumed`.
  - `NeedsInput` 문구: opencode는 "opencode 승인 요청: {message}"(200자에서 자름), pi의 `project_trust`는 "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다."
  - `v != 1`이거나 형식이 틀리면 빈 목록이다(Claude와 같다).
- 자산 공통 규칙
  - `process.env.DEVBOX_AGENT_ID`와 `DEVBOX_BIN`이 없으면 아무 hook도 등록하지 않는다.
  - 외부 패키지를 import하지 않는다. `node:child_process`만 쓴다.
  - `spawn(DEVBOX_BIN, ["agent","event","--from",<source>,JSON], {stdio:"ignore"})`로 띄우고, `error`를 무시하고, `unref()`한다. 절대 throw하지 않는다.
  - 시험용으로 `toDevboxEvent(event, state)` 순수 함수를 export한다.

**opencode 이벤트 매핑**

| opencode 이벤트 | 조건 | 보내는 것 |
|---|---|---|
| `session.created`·`session.updated` | `info.parentID` 있음 | 자식 세션으로 기억하고 보내지 않음 |
| `session.status` | busy·retry, 직전이 busy 아님, 루트 세션 | `working` + `sessionId` |
| `session.status` | idle, 직전이 busy, 루트 세션 | `idle` |
| `permission.asked` | 루트 세션 | `needsInput`, `message` = 권한 제목 |
| `permission.replied` | 루트 세션 | `inputResolved` |

fixture의 실제 필드 이름이 다르면 이 표를 고치고 검토에 알린다.

**시작 스크립트**
- 도구 줄 앞에 `export OPENCODE_CONFIG_CONTENT=<sh_quote(JSON)>`을 둔다. JSON 안의 경로는 file URL이며, 공백 같은 문자는 퍼센트 인코딩한다.
- 새 작업: `opencode --prompt="$(cat <prompt.md>)"`. `=` 형태라 `-`로 시작하는 프롬프트도 값으로 읽힌다.
- 재개: `opencode -s <id>`. 세션 id가 없으면 새로 시작한다.

**사용량**
- `opencode export <id>`를 stdin null, 10초 timeout, stdout 64 MiB cap으로 실행한다.
- assistant 메시지의 `tokens.input/output/cache.read/cache.write`와 `cost`를 더한다.
- `(세션, 마지막 TurnComplete 시각)`마다 한 번만 실행하고 결과를 cache한다.

**fixture 녹화**
1. 소유 임시 git 저장소와 tmux 전용 socket을 쓴다.
2. 이벤트를 임시 파일에 덧붙이는 녹화용 플러그인을 `OPENCODE_CONFIG_CONTENT`로 넣는다.
3. 두 번 실행한다:
   - "Reply with OK." 한 턴.
   - "Use the task tool to ask a subagent to reply OK, then reply OK." 자식 세션 이벤트를 얻기 위해서다.
4. 각각 `opencode export`를 저장한다.
5. 경로와 메시지 본문을 가려 필요한 필드만 남긴다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `profile::builtin_profiles_are_claude_codex_opencode_pi_in_order`: pi는 S7-8에서 채운다. 이 과제에서는 앞의 셋만 확인한다.
  - `profile::old_hook_values_still_parse`
  - `hooks::opencode_and_pi_sources_parse_from_cli_names`
  - `agent_event::cli_accepts_opencode_and_pi_sources`(`crates/cli/tests/agent_event.rs`)
  - `plugin_event::maps_working_idle_needs_input_and_resolved`
  - `plugin_event::first_session_id_emits_session_started`
  - `plugin_event::unknown_version_or_garbage_is_ignored`
  - `start::opencode_launch_exports_plugin_config_and_uses_prompt_equals`: 스크립트에 `export OPENCODE_CONFIG_CONTENT='{"plugin":["file:///d/agent-hooks/opencode-plugin-`가 있고 `opencode --prompt="$(cat '/d/prompt.md')"`가 있다.
  - `start::opencode_resume_uses_the_session_flag_without_a_prompt`
  - `start::prompt_with_a_leading_dash_quotes_and_dollar_is_passed_verbatim`
  - `usage_plugins::opencode_export_fixture_sums_assistant_tokens`
  - `opencodePlugin.test.ts`:
    - 녹화 이벤트 순서가 기대 payload 순서와 같다.
    - `child_session_events_are_ignored`
    - `plugin_is_inert_without_devbox_agent_id`: `spawn` mock이 불리지 않는다.
    - `spawn_errors_are_swallowed`
- [ ] 2. 시험이 실패해야 한다.
- [ ] 3. 구현한 뒤 `pnpm gen`을 돌린다.
- [ ] 4. `cargo test -p devbox-agents plugin_event start usage_plugins profile hooks`, `cargo test -p devbox-cli --test agent_event`, Vitest 해당 파일, `pnpm exec biome ci .`를 돌린다. 모두 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `feat(agents): support opencode as a builtin agent tool`.

## S7-8 pi 기본 지원

**계약** — pi 1.3.0 `--help`와 설치본 문서. 구현 시작 때 다시 확인한다.
- 사용법: `pi [options] [--] [@files...] [messages...]`.
  - `-e <path>`: 실행마다 확장을 하나 읽는다.
  - `--session-id <id>`: 그 id의 프로젝트 세션을 쓰고, 없으면 만든다.
  - `--`: 옵션 끝.
- `@`로 시작하는 인자는 파일 참조다. 그래서 프롬프트가 `@`로 시작하면 pi일 때만 공백 하나를 앞에 붙여 메시지로 넘긴다.
- 이벤트: `session_start`, `agent_start`, `agent_end`, `agent_settled`(최종), `turn_end`, `session_shutdown`, `project_trust`. 도구 실행 승인은 묻지 않는다.
- 세션은 `~/.pi/agent/sessions/` 아래 JSONL이다. 정확한 파일 이름 규칙은 녹화에서 확인한다. `usage{input,output,cacheRead,cacheWrite,totalTokens,cost}`.
- 확장 형태는 설치본 `…/pi-coding-agent/docs`의 확장 문서를 따른다.

**파일**
- 만들기 `crates/agents/assets/pi-extension.ts`
- 수정 `profile.rs`: builtin `pi`(label "pi", command "pi", hooks Pi).
- 수정 `prepare.rs`
  - 확장을 `write_artifact`로 쓴다.
  - 첫 실행 전에 UUID v4 세션 id를 만들어 작업에 바로 저장한다. 그래서 확장이 보고하지 못해도 재개할 수 있다.
- 수정 `start.rs`, `resume.rs`, `usage_plugins.rs`
- fixture `crates/agents/tests/fixtures/pi-1.3.0/{events.jsonl,session.jsonl}`
- 시험 `app/src/features/agents/harness/piExtension.test.ts`

**pi 이벤트 매핑**

| pi 이벤트 | 보내는 것 |
|---|---|
| `agent_start` | `working` + `sessionId` |
| `agent_settled` | `idle` |
| `project_trust` | `needsInput`, `message: "project_trust"` |

나머지 이벤트는 보내지 않는다. 신뢰 여부는 확장이 정하지 않고 사용자가 터미널에서 고른다.

**시작 스크립트**
- 새 작업: `pi -e <확장> --session-id <id> -- "$(cat <prompt.md>)"`.
- 재개: `pi -e <확장> --session-id <id>`.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `start::pi_launch_loads_the_extension_with_a_session_id_and_prompt_after_double_dash`
  - `start::pi_resume_reuses_the_session_id_without_a_prompt`
  - `prepare::prompt_starting_with_at_sign_is_prefixed_for_pi_only`: Claude·Codex·opencode의 프롬프트는 그대로다.
  - `agents::pi_session_id_is_assigned_before_the_first_launch`(`crates/cli/tests/agents.rs`)
  - `profile::builtin_profiles_are_claude_codex_opencode_pi_in_order`: 넷 모두 확인.
  - `usage_plugins::pi_session_fixture_sums_usage`
  - `piExtension.test.ts`:
    - `agent_start`·`agent_settled`가 working·idle이 된다.
    - `project_trust`가 needsInput이 된다.
    - `extension_is_inert_without_devbox_agent_id`
- [ ] 2. 시험이 실패해야 한다.
- [ ] 3. 구현한다. fixture 녹화는 S7-7과 같은 방식이며, 프롬프트는 "Reply with OK."다.
- [ ] 4. 관련 cargo 시험, Vitest, Biome를 돌린다. 모두 PASS여야 한다.
- [ ] 5. 검토 뒤 커밋: `feat(agents): support pi as a builtin agent tool`.

## S7-9 Shift+Enter·tmux 확장 키

**목표:** 터미널에서 Shift+Enter를 줄바꿈 키로 넘긴다. 요청하지 않은 프로그램에는 지금처럼 Enter로 간다.

**파일**
- 수정 `crates/terminal/assets/tmux.conf`. 아래 두 줄을 추가한다.
  - `-q`: 3.2–3.4에는 이 옵션이 없으므로 조용히 무시된다.
  - `always`는 쓰지 않는다.

  ```
  set -s extended-keys on
  set -sq extended-keys-format csi-u
  ```

- 수정 `app/src/features/terminal/TerminalView.tsx`: `attachCustomKeyEventHandler`.
  - keydown Enter이면서 shift만 눌렸을 때 `\x1b[13;2u`를 보내고 기본 동작을 막는다.
  - 그 밖의 키는 그대로 둔다.

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `TerminalView` 시험 `shift_enter_sends_csi_u_and_other_enters_are_unchanged`: Enter, Ctrl+Enter, Alt+Enter는 지금 동작 그대로다.
  - terminal crate `tmux_conf_enables_extended_keys_quietly`: 두 줄이 있고, format 줄이 `-sq`이며, `always`가 없다.
  - 소유 socket의 실제 tmux로 `show -s extended-keys`가 `on`인지 확인한다.
- [ ] 2. 시험이 실패해야 한다. 3. 구현한다. 4. 시험이 PASS여야 한다.
- [ ] 5. 실기는 S7-14 K1에서 확인한다.
  - pi에서 Shift+Enter는 줄바꿈, Enter는 보내기.
  - Claude·Codex·opencode의 실제 동작을 기록한다.
  - 일반 셸에서 Shift+Enter가 이상한 문자를 남기지 않는다.
- [ ] 6. 검토 뒤 커밋: `feat(terminal): pass Shift+Enter through tmux extended keys`.

## S7-10 I1 — 고아 자손이 도는 동안 사본 reader를 닫지 않는다

**문제**
- 직접 경로에서 bootstrap이 끝나 job handle이 모두 닫히면 job 이름이 사라진다.
- 그러면 고아 자손이 아직 돌고 있어도 `NotFound`가 "소비자 종료"로 처리된다(`windows_observation.rs:359-368`, `native.ps1:79-90`, `bootstrap.ps1:48`).

**설계 — holder + 영수증**
- **시작:** bootstrap.ps1은 job을 만든 뒤, 자기 자신을 job에 넣기 **전에** 별도 PowerShell holder를 job 밖에서 띄운다. 인자는 job 이름 `Local\Devbox.Observe.<nonce>`와 영수증 경로다.
- **준비 확인:** holder는 job handle을 쥔 뒤 `<nonce>.holder-ready`를 CreateNew로 쓴다. bootstrap은 이 표시를 본 뒤에만 job에 자기를 넣고, 명령을 실행하고, 자기 handle을 닫는다. holder 기동(PowerShell과 `Add-Type`)이 0.7–0.9초 걸리므로(PROGRESS §8 I2 실측), 기다리지 않으면 짧은 실행에서 holder가 job을 열기 전에 이름이 사라진다.
- **준비 실패:** 15초 안에 표시가 없으면 bootstrap은 명령을 실행하지 않고 정해진 오류 `windows.observer_unavailable`로 끝난다. holder 없이 진행하면 I1이 되살아나기 때문이다.
  - 제목: "Windows 작업을 시작하지 못했습니다(명령은 실행하지 않았습니다)."
  - 다음 행동: "다시 실행하세요. 계속되면 설정 → 앱 진단을 확인하세요."
- **holder 동작:**
  1. `OpenJobObject(JOB_OBJECT_QUERY)`로 handle을 쥔다. 이것으로 이름이 유지된다.
  2. 500 ms마다 `QueryInformationJobObject(JobObjectBasicAccountingInformation).ActiveProcesses`를 읽는다.
- **holder 종료 조건:**
  - (a) 한 번이라도 ≥1을 본 뒤 0이 되면 `<nonce>.emptied.json`(`{"nonce":…}`)을 CreateNew로 쓰고 끝난다.
  - (b) 60초 안에 ≥1을 못 보면 영수증 없이 끝난다.
  - (c) job을 열지 못하면 영수증 없이 바로 끝난다.
- **금지:** `SetInformationJobObject`는 쓰지 않는다(`crates/protocol/src/windows/tests.rs:142` 유지).
- **판정:**
  - `Empty`면 지금처럼 닫는다.
  - `NotFound`는 nonce가 맞는 영수증이 있을 때만 닫는다.
  - 영수증이 없으면 reader를 열어 둔다(fail-closed). `daemon/windows_observe.rs:22-28`의 native 재질문 대상에 "영수증 없는 NotFound"를 넣는다.
- **데몬 재시작:** 영수증은 private 폴더의 파일이라 재시작 뒤에도 같은 판정을 낸다. holder는 Windows 프로세스라 데몬과 상관없다.
- **판정 불가:** holder가 죽고 영수증도 없으면 기존 "확인 불가" 처리 경로를 그대로 쓴다. 새 화면은 만들지 않는다. 그 경로가 다음 행동을 알려 주지 않으면 문구만 보강한다.

**파일**
- `crates/protocol/src/windows/{bootstrap.ps1,native.ps1}`
- `crates/protocol/src/windows.rs`
- `crates/protocol/src/windows/observation.rs`(`ObservationControl`, `valid()`)
- `crates/runtime/src/windows_observation.rs`(`paths()`, `PrivatePaths`, :359-368)
- `crates/cli/src/daemon/windows_observe.rs`
- 시험: `crates/protocol/src/windows/tests.rs:123-148`, `crates/runtime/src/windows_copy_lifecycle_tests.rs`

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `bootstrap_starts_the_holder_before_job_assignment`: 스크립트 안 위치 순서를 비교한다. 포함 여부만으로는 부족하다.
  - `bootstrap_waits_for_holder_ready_before_assignment`: 준비 표시 대기가 job 넣기·명령 실행·handle 닫기보다 앞에 있다(위치 순서 비교).
  - `bootstrap_fails_with_observer_unavailable_when_the_holder_is_not_ready`: 15초 한도와 정해진 오류 코드.
  - `holder_queries_and_never_sets_job_information`
  - `holder_writes_the_receipt_with_create_new`
  - `not_found_without_receipt_keeps_the_reader_open`
  - `not_found_with_a_matching_receipt_closes`
  - `receipt_with_another_nonce_is_ignored`
  - `empty_still_closes`
  - `receipt_decision_survives_a_daemon_restart`
- [ ] 2. `cargo test -p devbox-protocol windows`와 `cargo test -p devbox-runtime windows_copy`에서 새 시험이 실패해야 한다.
- [ ] 3. 구현한다. 4. 같은 시험이 PASS여야 한다. Windows strict clippy는 S7-14의 Windows CI 근거로 확인한다.
- [ ] 5. 실기는 S7-14 W1에서 한다. 실행하지 못하면 I1은 1.0.0 공개를 막는 NOT_RUN으로 남는다.
- [ ] 6. 검토 뒤 커밋: `fix(runtime): keep Windows copy readers open while orphans run`.

## S7-O 선택 과제 — §2.2에서 승인된 것만

각 과제는 위와 같은 순서(실패 시험 → 구현 → PASS → 검토 → 커밋)를 따른다.

**O1a — 재시작 빠른 길**
- 범위: `crates/cli/src/setup.rs:296-345`
  - `publish()`가 내용이 바뀌었는지 돌려주게 한다.
  - restart 조건: unit이나 env가 바뀜, 서비스가 비활성, 또는 실행 중인 MainPID의 `/proc/<pid>/exe` canonical 경로가 이 실행 파일과 다름.
  - 아니면 단계 보고를 "건너뜀(변경 없음)"으로 남긴다.
- 문구: ConnectionGate와 README :17·:40을 고친다.
- 시험(가짜 runner):
  - `unchanged_install_skips_the_daemon_restart`
  - `changed_binary_restarts`
  - `changed_unit_restarts`
  - `inactive_daemon_restarts`
- 커밋: `perf(setup): restart the daemon only when its install changed`.

**O1b — N1 Windows 실행 브리지 재연결**
- 범위: `crates/cli/src/windows_exec.rs:109-133`
  - `accepted()`는 `Result<bool, ClientError>`를 돌려준다.
  - 연결 오류가 나면 250 ms에서 5초까지 늘려 가며 재연결하고 Hello를 다시 보낸다. 실행이 끝날 때만 그만둔다.
- 시험: `ready_poll_reconnects_after_a_daemon_restart`. 가짜 socket server를 닫았다가 다시 연다.
- 커밋: `fix(cli): reconnect Windows run bridges after daemon restarts`.

**O2 — Codex rollout 꼬리 읽기**
- 범위: `crates/agents/src/tracker.rs`
  - `session_meta`는 첫 줄에서 읽고, 첫 줄 한도는 1 MiB다.
  - 턴 기록은 파일 끝 8 MiB 창에서 읽는다. 창의 첫 부분 줄은 버린다.
  - 128 KiB를 넘는 줄은 건너뛰고 수만 센다. 오류로 보지 않는다.
- 시험(`tracker_tests.rs`):
  - `large_rollout_finds_the_latest_turn_in_the_tail`
  - `oversized_line_is_skipped_not_fatal`
  - `session_meta_over_128k_is_read`
- 커밋: `fix(agents): track long Codex rollouts by reading their tail`.

**O3 — EnvEditor 늦은 저장**
- 범위: `app/src/features/tools/api/EnvEditor.tsx:149`의 `path` 비교를 `load()`(:92-104)와 같은 revision 비교로 바꾼다.
- 시험: `late_save_of_a_does_not_overwrite_new_env_b_draft`.
- 커밋: `fix(tools): keep environment drafts separate when a save finishes late`.

**O4 — 소유 자원 강등**
- 범위: `crates/agents/src/prepare.rs:334-346`. 기존 binding이 `Owned`이고 관찰한 경로·branch·등록이 모두 맞으면 `Owned`를 유지한다. 그 밖에는 지금처럼 `Partial`이다.
- 시험: `reconcile_keeps_owned_resources_after_session_loss`. 이어지는 정리가 worktree와 branch를 지워야 한다.
- 커밋: `fix(agents): keep owned resources owned when preparation is interrupted`.

**O5 — 폐기 스냅샷**
- 범위: `crates/agents/src/discard_snapshot.rs`
  - Git 무시 항목은 내용 해시 대신 메타데이터(크기, mtime ns, mode, inode)로 비교한다.
  - 해시 계산은 lifecycle 잠금 밖에서 하고, 잠금 안에서는 메타데이터로 다시 확인만 한다.
- 시험:
  - `ignored_files_are_compared_by_metadata_without_reading_them`
  - `tracked_change_after_confirmation_still_blocks_discard`
  - `discard_holds_the_lock_only_for_verification`
- 커밋: `perf(agents): snapshot ignored files by metadata when discarding`.

**O6a — 새 작업 창**
- 범위: `NewTaskDialog.tsx`
  - 프로젝트별 마지막 도구를 기억한다. localStorage key는 `devbox.newTask.lastProfile.<projectId>`다.
  - 브랜치 이름 칸을 둔다. 기본값은 제목 slug이고 한글 제목이면 빈칸이다. 검사는 `git check-ref-format` 규칙을 따르고, 데몬 `plan.rs`에서 다시 검증한다.
- 시험(Vitest + `plan.rs`):
  - `remembers_the_last_tool_per_project`
  - `korean_title_requires_a_branch_name`
  - `invalid_branch_name_is_rejected_by_the_daemon`
- 커밋: `feat(agents): remember tools per project and name task branches`.

**O6b — 첫 실행과 정리 오류**
- 범위:
  - `NativeMainController.tsx:125-127`의 [나중에]가 `autostartAsked`를 저장한다.
  - `AgentHeader.tsx:72`의 원문 `cleanupError`를 정해진 코드 문구로 바꾸고, 원문은 [자세히] 아래에 둔다.
- 시험:
  - `later_is_remembered_across_restarts`
  - `cleanup_error_shows_cause_and_next_action`
- 커밋: `fix(app): remember first-run choice and explain cleanup failures`.

## S7-11 문서 P0 — rc.2 설치 파일과 함께 나가는 문서

**파일과 결정**

`README.md`
- **상태:** 정식 1.0.0 준비 중이며 릴리스 후보는 `v1.0.0-rc.2`(prerelease)다. RC는 앱 업데이트가 꺼져 있어 다음 버전은 설치 파일로 직접 설치한다.
- **요구 사항:**
  - Windows 11, systemd가 켜진 WSL2.
  - tmux 3.2 이상. Shift+Enter는 실행 중인 tmux 서버가 3.5 이상일 때만 CSI-u로 보내고, 3.2–3.4에서는 Enter로 동작한다(pi 1.3.0 `docs/tmux.md`로 정했던 "3.2–3.4는 xterm `modifyOtherKeys`"는 S7-13 Fix F가 정정했다. 3.2–3.4가 확장 키를 요청하지 않은 프로그램에 CSI-u를 전하지 않는다는 것은 tmux 3.4 소스를 읽어 확인했고 실기는 S7-14 K1이 한다).
  - git. PR 기능은 gh 로그인이 필요하다.
  - 쓸 에이전트 CLI(claude·codex·opencode·pi)를 설치하고 로그인한다.
- **설치:**
  - 설치 파일은 `devbox-<버전>-setup.exe`이고 `Get-FileHash`로 SHA-256을 확인한다.
  - 서명이 없어 SmartScreen 경고가 날 수 있다. 실제 문구는 S7-14에서 보고 반영한다.
- **에이전트 도구:** 기본 4종 표(상태 감지 방식, 재개, 토큰 합계). 사용자 정의는 `docs/config.md`.
- **MCP 4종:** 등록 명령은 S7-14 전에 설치본에서 다시 확인한다.
  - Claude: `claude mcp add -s user devbox -- ~/.local/share/devbox/bin/devbox mcp`
  - Codex: `codex mcp add devbox -- ~/.local/share/devbox/bin/devbox mcp`
  - opencode: 설정의 `"mcp": {"devbox": {"type": "local", "command": ["/home/<사용자>/.local/share/devbox/bin/devbox", "mcp"], "enabled": true}}`
  - pi: 1.3.0 문서의 등록 방식.
  - 쓰기 도구는 `[mcp] write = true`일 때만 켜진다. accept 채널 경로는 `~/.local/share/devbox-accept/bin/devbox`다.
- **백업·이동:**
  - 복구 문자열을 보관한다.
  - config.toml은 dotfiles로 관리한다. local 파일은 제외한다.
  - vault는 Git으로 관리한다. devbox.db와 활동 기록은 PC별이다.
- **제거:** Windows 앱 제거, [WSL 쪽 정리], 그 뒤 남는 경로.
- **알려진 제한:** SC2 cold, SC7, NOT_RUN 요약. 자세한 내용은 PROGRESS를 가리킨다.
- **경로 찾기 안내:** 셸이 답한 결과만 5분 동안 다시 쓰고, 환경 점검은 매번 셸을 실행한다. 셸이 답하지 않으면 그 작업은 서비스 PATH 결과로 시작하고 다음 작업 시작 때 셸에 다시 묻는다(S7-11 정정).

`docs/config.md` 만들기
- 대상: config.toml, config.local.toml, devbox.toml의 키, 병합 규칙.
- 오류 동작: S7-2 정책 표를 그대로 옮긴다.
- 예시:
  - `[[agents.profiles]]`: `hooks` 값 5종, `waitingPatterns`와 별칭.
  - `[[schedule]]`: `id`가 필수다.
  - Windows `settings.json`.
- 시험 `crates/cli/tests/docs_config_examples.rs`: 문서 안의 `toml` 코드 블록을 모두 실제 읽기 함수로 파싱해 오류가 없어야 한다.

`SECURITY.md` 만들기
- 범위: ADR 0016의 개인용 범위.
- 활동 기록: 동의 전 수집 없음, 일시중지, 앱 이름·창 제목, 저장 위치, 외부 전송 없음.
- 비밀: 저장 방식과 복구 문자열.
- 기본으로 꺼진 것: MCP 쓰기, 웹훅 LAN.
- 지원 번들의 가림.
- 서명 없는 설치 파일.
- 신고 경로: D2 답에 따른다.

`01-design.md`
- :3-4 상태 줄.
- §2.1·§2.4(:99-104·:159): 하네스 4종 기본 지원과 연결 방식.
- R5(:1022): 실제 위치.
- §7: `[[schedule]]`의 `id`, :529-530 저장 위치는 Windows `settings.json`.
- :932·:937: prerelease 업데이트 꺼짐, 태그 규칙.
- :962: 리뷰는 CONVENTIONS §8.

그 밖
- `app/README.md:57`: rc 태그 규칙.
- `app/acceptance/README.md:22`: `loc.sh 60000`을 `200000`으로.

**단계**
- [ ] 1. `docs_config_examples` 시험을 먼저 쓰고 실패를 확인한다. 파일이 없어서 실패한다.
- [ ] 2. 문서를 쓴다. 3. 시험이 PASS여야 한다.
- [ ] 4. 문서 검토 에이전트가 확인할 것:
  - 문장마다 코드·계약 근거가 있는지.
  - 상대 링크가 깨지지 않았는지.
  - ui-terms를 지켰는지.
- [ ] 5. 커밋: `docs: document rc.2 install, agent tools, MCP, config and security`.

## S7-12 MIT·제3자 고지

**파일**
- 만들기 `LICENSE`: MIT, `Copyright (c) 2026 jihoon22-lee`. 이메일은 넣지 않는다.
- 라이선스 메타데이터
  - `Cargo.toml` `[workspace.package] license = "MIT"`, 각 workspace crate에 `license.workspace = true`.
  - `app/package.json` `"license": "MIT"`, `app/src-tauri/Cargo.toml` `license = "MIT"`.
- 만들기 `xtask/src/notices.rs`, 수정 `xtask/src/main.rs`: `cargo run -p xtask -- notices` 하위 명령.
- 만들기 `THIRD_PARTY_NOTICES.md`: 생성물이다.
- 수정 `app/src-tauri/tauri.conf.json` `bundle.resources`: `LICENSE`와 `THIRD_PARTY_NOTICES.md`를 설치 폴더에 넣는다.

**생성기 결정**
- Rust 쪽은 `cargo metadata --format-version 1 --locked --offline`을 루트와 `app/src-tauri` 두 manifest에 돌린다. normal 의존성만 넣는다(dev·build 제외).
- npm 쪽은 `pnpm --filter app licenses list --prod --json`이다.
- 패키지마다 원본 폴더의 `LICENSE*`·`COPYING*`·`NOTICE*` 본문을 넣는다. 같은 본문은 하나로 묶고 패키지 목록을 붙인다. 파일이 없으면 SPDX 식을 적는다.
- 머리에 입력 지문을 둔다: `<!-- inputs: Cargo.lock=<sha256> app/src-tauri/Cargo.lock=<sha256> pnpm-lock.yaml=<sha256> -->`. S7-13 영역 6 뒤에는 `manifests=<sha256>`(작업 공간 root와 모든 멤버, 앱 crate, 앱 package의 manifest)와 `bundled=<sha256>`(함께 싣는 글꼴의 라이선스 본문)도 들고, S8-2c 뒤에는 `linked=<sha256>`(링크되는 코드의 라이선스 본문과 표의 각 행)도 든다. 앱에 내장된 D2Coding 글꼴은 어느 lock 파일에도 없어 `xtask/src/notices/bundled.rs`의 표로 고지에 더한다. toolchain과 SDK가 링크하는 코드도 어느 lock 파일에도 없어(S8-2c 뒤) `xtask/src/notices/linked.rs`의 표(`LINKED`)와 `xtask/notices/linked/`의 본문으로 더한다.
- 신선도 시험은 `xtask/src/notices.rs`의 `#[test] notices_inputs_are_current`다.
  - 세 lock 파일의 SHA-256을 다시 계산해 지문과 비교한다(영역 6 뒤에는 manifest와 글꼴 라이선스도, S8-2c 뒤에는 링크되는 코드의 라이선스도). 네트워크는 쓰지 않는다.
  - CI의 `cargo test --workspace`가 돌리므로 `.github` 줄을 늘리지 않는다.
  - 실패 문구: "THIRD_PARTY_NOTICES.md가 입력(lock 파일, manifest, 함께 싣는 글꼴과 링크되는 코드의 라이선스)과 다릅니다. `cargo run -p xtask -- notices`로 다시 만드세요."(S8-2c 뒤. S7-13 영역 6 뒤에는 "입력(lock 파일, manifest, 함께 싣는 글꼴의 라이선스)"였고, 처음에는 "lock 파일과 다릅니다"였다)

**단계**
- [ ] 1. 실패 시험을 쓴다.
  - `notices::groups_packages_by_identical_license_text`: fixture metadata JSON으로.
  - `notices::missing_license_file_falls_back_to_spdx`
  - `notices_inputs_are_current`
- [ ] 2. 시험이 실패해야 한다.
- [ ] 3. 구현하고 생성한다.
- [ ] 4. `cargo test -p xtask`가 PASS여야 한다. 설치 폴더 포함 여부는 S7-14 N1에서 확인한다.
- [ ] 5. 검토 뒤 커밋: `chore: license under MIT and ship third-party notices`.

## S7-13 브랜치 전체 검토·rc.2 버전·전체 검사

- [ ] 1. 브랜치 전체 독립 검토(CONVENTIONS로 옮긴 00 §3:159). 6개 영역을 각각 새 검토 에이전트가 맡는다.
  1. agents·hooks·하네스
  2. daemon·설정·setup·CLI
  3. runtime·Windows·PowerShell
  4. 화면
  5. native(`app/src-tauri`)
  6. 문서·고지·CI 범위

  지적마다 수정 과제와 별도 검토를 거친다. 결과는 PROGRESS에 남긴다.
- [ ] 2. 버전을 `1.0.0-rc.2`로 바꾼다.
  - 대상: `Cargo.toml:7`, `app/package.json:4`, `app/src-tauri/tauri.conf.json:4`, `app/src-tauri/Cargo.toml:3`.
  - 두 manifest에 `cargo update --workspace --offline`을 돌린다.
  - `pnpm install --frozen-lockfile --offline`이 통과해야 한다.
  - `scripts/tests/test_release.py`의 버전 고정 fixture를 확인한다.
  - 버전 원장 일치 검사를 한다(07c S6-6a와 같은 방식). 런타임에 포함되는 경로에 `1.0.0-rc.1`이 0건이어야 한다.
  - manifest(`Cargo.toml`, `app/src-tauri/Cargo.toml`, `app/package.json`)와 Cargo lock 둘(`Cargo.lock`, `app/src-tauri/Cargo.lock`)이 바뀌므로 같은 커밋에서 `cargo run -p xtask -- notices`로 고지를 다시 만든다. `cargo test -p xtask notices`가 PASS여야 한다.
  - `pnpm exec biome ci .` 뒤 커밋: `chore(release): set version to 1.0.0-rc.2`.
- [ ] 3. `python3 scripts/verify-resources.py --wait -- pnpm check`를 한 번 돌린다. 실패하면 원본을 보존하고 실패·직접 영향 범위만 고쳐서 다시 돌린다. 이 수정 커밋은 버전 커밋 뒤에 와도 된다.
- [ ] 4. 이 시점의 HEAD를 Windows 후보 SHA로 고정한다. 후보 SHA에는 rc.2 버전이 들어 있어야 한다(U1이 버전 불일치 경로를 실제로 거치게 하기 위해서다). 이후 제품 입력이 바뀌면 새 후보로 S7-14의 영향 시나리오를 다시 돈다.

## S7-14 Windows 후보·accept 수용

**준비**
- [x] 1. rc.1 기준선(업그레이드 시험용). accept 채널 빌드는 PR·dispatch 실행에만 있고, rc.1의 release 브랜치는 지워졌다. 그래서 백업형 push를 한다(PR 없음, CI 산출물 예외).
  1. `git push origin 28eb2a91f3…:refs/heads/v1/s7-rc1-accept`(정확한 40자 SHA는 `git rev-parse 28eb2a91`).
  2. `bash scripts/dogfood.sh v1/s7-rc1-accept --channel accept`. 실행이 없으면 dogfood가 windows.yml을 dispatch한다. SHA·run·채널·해시를 확인한다.
  3. dispatch는 `scripts/ci-scope.sh`에서 늘 `code=true`이고 accept 채널도 함께 포장하므로 기준선 산출물이 만들어진다(2026-10-27 확인).
- [x] 2. 후보.
  1. `git push -u origin v1/s7-rc2`. 백업 push이며 PR은 열지 않는다.
  2. `gh workflow run windows.yml --ref v1/s7-rc2`
  3. `bash scripts/dogfood.sh v1/s7-rc2 --channel accept`
  4. Windows strict clippy·build 근거는 그 run의 결과다.
- [x] 3. harness를 복원한다.
  1. Git common-dir `devbox-archive/cleanup-20261026/local-verification-20261026.tar.gz`의 SHA-256이 `a292415367811796f7f16efcf5ce2b02eb69be4e2e76b08ca0d1e67ea8afecea`인지 확인한다.
  2. `devbox-verification/s6-6b/acceptance/harness/`만 소유 임시 폴더 `/tmp/devbox-s7-acceptance-<난수>/`에 푼다.
  3. 결과는 common-dir `devbox-verification/s7/`에 bounded JSON·로그로 남긴다.

**시나리오** — 결과는 PASS·FAIL·NOT_RUN과 근거 경로로 남긴다.

| ID | 확인 | 연결 |
|---|---|---|
| U1 | accept rc.1을 설치하고 claude·codex 작업을 짧은 프롬프트로 시작한다. rc.2 설치 파일을 덮어 실행한다. 확인할 것: 두 세션 유지, 기존 claude 세션의 다음 턴이 Idle로 반영됨, 새 claude 작업이 `differs` 없이 시작됨. 이전 세션에서 이미 떠 있던 `devbox mcp`(rc.1 실행 파일)가 업그레이드 뒤 어떻게 답하는지도 기록만 한다. 응답에 새 값 `opencode`·`pi`가 섞일 수 있다. 고치지는 않는다 | B1, SC4 |
| U1 추가 | rc.1 Claude 작업(rc.1 hook 설정)이 rc.2 설치 뒤 승인이 필요한 명령을 만난다. 터미널에서 승인한 뒤에도 그 턴이 끝날 때까지 보내기가 "에이전트가 선택을 기다리고 있습니다."로 막히는지, 턴이 끝나면 풀리는지, 작업을 재개하면 새 hook 설정으로 회복하는지 기록한다(README에 적은 동작과 같아야 한다) | 영역 1 M2 |
| H1–H4 | claude·codex·opencode·pi로 새 worktree 작업을 하나씩 만든다. 프롬프트는 "Reply with OK and stop."이다. 확인할 것: 실행 중 → 턴 종료가 2초 안에 목록과 Windows 알림에 반영(실측), 재개 1회, 토큰 합계, 환경 점검 4종 Ok | SC3, B4 |
| H1 추가(신뢰 대화) | claude로 새 worktree 작업을 만들어 폴더 신뢰 대화가 나타나는지 기록한다. 나타나면 대기로 잡히고 화면·MCP 보내기가 막히는지 확인한다(`trust.txt` fixture는 문자열 기반이다) | 영역 1 M3 |
| H1 추가(셸 모드) | claude 입력창에 `!`로 시작하는 해 없는 초안(예: `! echo ok`)을 두고 Enter는 누르지 않는다. footer가 마지막 줄들에 `! for shell mode`를 보이고 화면·MCP 보내기가 거절된다. 실제 화면의 줄 배치를 `shell-mode.txt`(문자열 기반)와 대조한다 | Fix A 5회차 |
| H1 추가(? 도움말) | claude의 `?` 도움말 화면을 캡처해 `! for shell mode`가 마지막 6줄 안에 드는지, 도움말이 열려 있는 동안 보내기가 거절되는지 기록한다(문서는 "막힐 수 있다"로만 적었다) | Fix A 5회차 |
| H2 추가 | codex 새 worktree의 신뢰 화면이 대기로 잡힌다 | S7-5 |
| H2 추가(재개) | 목표(goal)가 멈춘 codex 작업을 재개한 뒤 보낸다. 실제 200×50 화면을 캡처하고, 가운데 확인 상자가 대기로 잡혀 보내기가 거절되는지 기록한다 | Fix A 1회차 |
| H2 추가(재개 직후) | `codex resume` 직후 1초 안에 보내고 어떻게 되는지 기록한다(시작 창 경쟁, S7-8 유보와 같은 부류) | Fix A 2회차 |
| H2 추가(/feedback) | 실제 `/feedback` 화면에서 보내기가 거절되는지 확인한다. 피드백은 제출하지 않는다 | Fix A 2회차 |
| H2 추가(실화면) | 실제 codex 작업에서 다음 화면을 하나씩 열어 대기로 읽히는지, 보내기가 거절되는지 확인한다. 아무것도 제출하거나 저장하지 않는다: 텍스트 입력 상자(`/rename` 또는 목표 편집), `/statusline` 또는 `/experimental`, `/keymap`과 편집, 모델의 비동기 질문, 재개 선택 목록, 인라인 기록 보기(Ctrl+T 뒤 Esc로 탐색). 전체 화면 기록의 프롬프트 탐색은 보내기를 막지 않는지도 확인한다 | Fix A 3회차 |
| H2 추가(외부 편집기) | codex에서 Ctrl+G로 GUI 외부 편집기를 연다. 편집기가 떠 있는 동안 붙여 넣은 글이 어디로 가는지, footer의 `Save and close external editor to continue.`가 보이는지 기록한다(devbox는 이 상태를 알아보지 않는다) | Fix A 4회차 |
| H2 추가(셸 모드 표시) | codex 입력창에 `!` 초안을 둔 채 터미널 폭을 줄이거나 /side 대화를 열거나 단축키 도움말을 연다. `Shell mode` 표시가 그려지지 않는 경우와 그때의 보내기 반응을 기록한다 | Fix A 4회차 |
| H3 추가 | `opencode debug config`에 사용자 플러그인과 devbox 플러그인이 함께 보인다 | 검토 초점 5 |
| H3 추가(환경) | 실제 opencode TUI 작업에서 bash 도구로 `env \| grep DEVBOX_`를 실행한다. `DEVBOX_AGENT_ID`와 `DEVBOX_AGENT_LAUNCH`는 없고 `DEVBOX_BIN`과 `DEVBOX_INSTANCE`는 있다. `!env`로도 같은지 확인하고, 그 밖에 env 옵션 없이 띄우는 프로세스(외부 편집기 등)가 `DEVBOX_AGENT_ID`를 보는지 기록한다 | Fix B |
| H3 추가(보고) | 같은 작업의 보고(실행 중·대기·Idle)가 그 작업의 시작(launch)에 묶여 도착해 목록의 상태가 갱신된다 | Fix A 1회차, Fix B |
| H3 추가(중첩 실행) | 작업의 bash 도구에서 `opencode run "Reply with OK."`를 실행한 뒤 그 작업의 sessionId와 상태가 그대로인지 확인한다 | Fix B |
| H3 추가(셸 모드) | opencode 입력창에서 `!`로 셸 모드를 켠다. 힌트 줄이 `esc exit shell mode`를 보이고 화면·MCP 보내기가 거절된다. 세션이 `retry` 상태일 때 힌트 줄이 사라지는지, 붙여 넣은 `!`가 셸 모드를 켜는지 기록한다(패턴은 설치본 문자열 기반이다) | Fix A 4회차 |
| H3 추가(exit) | opencode 작업에 정확히 `exit`, `quit`, `:q`를 하나씩 보내 어떻게 되는지 기록한다(종료 여부, `-s <id>`로 재개되는지). 거절할지는 이 결과를 보고 정한다 | Fix A 4회차 |
| P1 | claude에 승인이 필요한 명령을 요청한다. 확인할 것: 대기 표시, 화면 보내기 비활성, MCP 보내기 거절, pane 입력 0. 터미널에서 승인 → 실행 중 → 턴 종료. 붙여넣기+Enter의 실제 반응도 기록한다 | B3 |
| P1 추가(Codex 승인) | codex에 승인이 필요한 명령을 요청한다(exec 승인 overlay). 확인할 것: 대기 표시, 화면 보내기 비활성, MCP 보내기 거절, pane 입력 0 | Fix A 1회차 |
| SN1 | Changes 탭에서 form feed(^L) 같은 제어 문자가 든 줄을 인용해 에이전트에 보내거나, 색 코드가 든 터미널 출력을 입력창에 붙여 보낸다. 보내기가 거절되고 초안이 남으며 메시지가 지울 것을 말한다 | Fix A 4회차 |
| SN2 | 보이지 않는 글자(U+200B, U+2060)를 앞에 붙인 `!echo devbox-ok`를 각 도구 작업에 앱과 MCP로 보내, 도구가 셸 명령으로 실행하는지 기록한다(거절도 trim도 되지 않는다. 기록만 하고 고치지 않는다) | Fix A 4회차 |
| C1 | accept 설정 `~/.config/devbox-accept/config.toml`을 소유 사본으로 백업하고 문법을 깨뜨린 뒤 앱을 다시 연다. 확인할 것: 기능 저하 안내, 파일 바이트 불변. 원복 뒤 정상 | B2 |
| K1 | tmux 3.5 이상 호스트에서 pi는 Shift+Enter로 줄바꿈하고 Enter로 제출한다. claude·codex·opencode의 반응을 기록하고, 일반 셸은 Shift+Enter로 줄을 실행하며 이상한 문자를 남기지 않는다. 3.4 이하는 아래 K1 추가(3.4)에서 본다 | S7-9, Fix F |
| K1 추가(3.4) | tmux 3.4 호스트(Ubuntu 24.04 기본)에서 bash·claude·codex·opencode·pi 작업을 연다. Shift+Enter가 Enter처럼 동작하고(줄이 실행되거나 입력창이 제출되며, 삼켜지거나 이상한 문자가 남지 않는다) pi는 줄바꿈 대신 제출한다. 터미널 attach 결과의 `extendedKeys`는 `false`다. 호스트가 없으면 NOT_RUN이다. CI(ubuntu-24.04의 tmux 3.4)는 `false`까지만 보여 준다 | Fix F |
| K1 추가(3.5 경계) | tmux 3.5 또는 3.5a 호스트가 있으면 K1과 같은 확인을 한다. 3.5 경계는 tmux 소스를 읽어 정했고, 실제로 돌려 본 것은 이 PC의 tmux 3.6뿐이다(CI의 3.4는 서버 버전 질의 시험까지만 돈다) | Fix F |
| K1 추가(WebView2) | tmux 3.5 이상에서 취소된 keydown 뒤 `keypress`가 막히는지, Shift+NumpadEnter도 같은 길로 가는지 확인한다 | Fix F |
| K1 추가(보내기 경로) | pi가 확장 키를 켠 뒤에도 devbox의 보내기(`paste-buffer -p` 뒤 `send-keys Enter`)가 pi에서 제출되는지 확인한다 | Fix F |
| K1 추가(tmux 업그레이드) | devbox tmux 서버가 떠 있는 채로 호스트 tmux를 3.4에서 3.5 이상으로 올린다. 새로 연 터미널에서도 그 서버가 다시 시작될 때까지 Shift+Enter는 Enter로 동작하고(`extendedKeys`가 `false`), 다시 시작한 뒤에는 CSI-u로 간다. 호스트 tmux를 바꾸는 것은 사용자 PC 설정 변경이므로 먼저 묻고, 못 바꾸면 NOT_RUN이다 | Fix F 1회차 |
| W1 | Windows 실행 두 가지. (가) `cmd /c start "" /b powershell -NoProfile -Command "Start-Sleep 20"`(부모 즉시 종료): 고아가 도는 동안 reader 유지, 끝난 뒤 영수증과 닫힘. (나) `cmd /c exit 0`(아주 짧은 실행): 정상으로 닫힌다. holder가 실행마다 더하는 시작 지연을 raw로 기록한다 | I1 |
| W1 (다) | Windows 실행으로 빌드 서버를 남기는 빌드를 한다(`dotnet build`, 없으면 `pnpm tauri build`). 파일을 바꿔 다시 실행하면 실행 화면이 바뀐 제목과 다음 행동("끝날 때까지 기다린 뒤 …")을 모두 보이는지 확인하고, 서버별 붙잡는 시간과 오류를 기록한다. `dotnet build-server shutdown`·`gradle --stop`(또는 작업 관리자) 뒤에는 다음 실행이 시작된다 | Fix C |
| W1 (라) | `#[ignore]` 실제 Windows 시험 `the_holder_receipt_waits_for_an_orphan_that_outlives_the_bootstrap`를 그 doc comment의 두 단계로 돌린다(빌드는 기본 잠금, 실행은 `windows-acceptance` 잠금과 `DEVBOX_TEST_WINDOWS_POWERSHELL`·`DEVBOX_TEST_WINDOWS_DIR`). PASS는 이 실행으로만 적는다 | Fix C |
| W1 (마) | Windows 작업을 정지하고 수정한 뒤 다시 시작한다. busy 거절이 없어야 한다 | Fix C 1회차 |
| W1 (바) | Windows 실행과 붙잡힌 reader의 probe 모두 콘솔 창을 열지 않는다. 출력·종료 코드·정지·job 배정·UTF-8 출력은 그대로 동작한다 | Fix C, Fix E 7b |
| W1 (사) | Windows 실행의 `windows.prepare`부터 `assigned`까지 cold 시간을 잰다. 35초 안이어야 한다 | Fix C |
| W1 (아) | 붙잡힌 reader의 probe가 대략 분당 한 번인지, holder를 작업 관리자에서 끝낸 뒤 약 70초 안에 풀리는지 확인한다 | Fix C |
| W1 (자) | 느리지만 성공하는 holder 시작에서도 그 실행의 활동이 공유 시작 한도(35초) 안에 알려지고, 그 뒤 앱 안 업데이트 설치가 `nativeUnknown`으로 막히지 않는지 확인한다 | Fix C 1회차 |
| CS1 | `systemctl --user kill -s STOP devbox-accept@accept.service` 뒤 앱을 끝내고 다시 실행한다. 앱 안에서 어느 길이 회복하는지 기록한다: 약 30초 뒤 연결 배너의 [다시 시도](5회 실패 뒤), 설정 → 앱 진단의 [다시 설정]. 배너가 나오기까지 걸린 시간을 적고, 앱이 이미 연결된 상태에서 STOP을 보낸 경우도 본다. 마지막에 `kill -s CONT` | Fix E 1–2, 영역 5 확인 1 |
| CS2 | [다시 설정]과 [다시 시도](연결 화면과 연결 배너)가 각각 MainPID를 바꾼다. 배포판 선택은 MainPID를 바꾸지 않는다(선택 화면에 닿을 수 있을 때만: 사용자 배포판이 둘 이상이고 accept 앱의 `settings.json`에 배포판이 없을 때) | Fix E 3, 영역 5 확인 2 |
| CS3 | 바뀐 것이 없는 일반 재실행은 MainPID를 유지하고 setup의 재시작 단계 메모가 "건너뜀(변경 없음)"이다 | O1a, 영역 5 확인 3 |
| CS4 | 시작 doctor(`devbox doctor --json --startup`)는 시작 중에 CLI 조회 셸(`bash -lic … devbox-probe`, S7-14 전에는 `bash -ic`)이나 에이전트 `--version`을 실행하지 않는다(읽기 전용 `pgrep -af devbox-probe` 반복으로 보거나 ready까지 시간을 rc.1과 비교한다). 환경 점검과 설정 → 에이전트는 `tool.claude`·`tool.codex`·`tool.opencode`·`tool.pi`를 계속 보인다 | Fix E 7 |
| CS5 | rc.1 → rc.2 제자리 업그레이드: 첫 실행 뒤 `/proc/<MainPID>/exe`가 rc.2의 `devbox-<sha>`이고, rc.1 세션의 hook이 여전히 데몬에 닿는다(U1) | 영역 5 확인 4 |
| CS6 | [나중에](버튼·Esc·바깥 클릭): accept 앱의 HKCU `…\CurrentVersion\Run` 값이 그대로이고 `settings.json`에 `autostart_asked: true`가 있으며 다시 열어도 대화상자가 없다 | O6b, 영역 5 확인 5 |
| CS7 | 실패한 연결 화면의 [자세히]가 journal 줄을 보인다. 비었거나 읽을 수 없는 사용자 journal이 무엇을 보이는지 기록한다 | 영역 5 확인 6 |
| CS8 | Windows CI의 `cargo test --lib`가 `platform::supervisor::tests`와 `platform/native_activity/native_tests.rs`를 포함해 돈다(CI 로그로 확인한다) | 영역 5 확인 8 |
| CS9 | 느리지만 정상인 데몬 시작(setup과 첫 Welcome): 연결 배너의 [다시 시도](실패 5회, 약 29초 뒤)가 Welcome보다 먼저 나타나는지 기록한다. 누른 뒤에는 4회 만에 다시 나올 수 있다 | Fix E |
| CS10 | accept 앱의 `settings.json` 사본을 따로 두고 `{`를 써 넣은 뒤 다시 연다. 연결 화면이 "앱 설정 파일을 읽지 못했습니다"와 전체 경로를 보이고, [자세히]가 "실패한 단계 앱 설정 읽기"와 파서 문구를 최근 로그 없이 보인다. 파일은 그대로이고(해시 전후 비교) 옮겨지지 않는다. 사본을 되돌린 뒤 [다시 시도]하면 정상으로 시작한다 | Fix E 6 |
| CS11 | accept 인스턴스만으로 [다시 설정] 실패를 만들 수 있으면, 실패한 단계의 문구가 앱 진단의 "작업을 완료하지 못했습니다…" 아래와 시작 문제 배너에 나온다 | Fix E 5 |
| CL1 | 정리 명령이 폴더에 파일을 쓰는 실제 프로젝트(`[agent] teardown`)로 자동 정리, 설정 → 에이전트의 수동 정리, 버리기를 각각 한다. 정리 전에 깨끗했던 폴더는 정리 명령이 만든 변경이 되돌려진 뒤 폴더와 브랜치가 지워지고, 정리 명령이 파일을 쓰고 실패해도 작업이 정리 후보에 남는다. 버리기는 그 파일 때문에 `agents.state_changed`로 거절되지 않는다 | Fix D 1–3차 |
| CL2 | 스쿼시 병합(또는 PR)으로 닫고 정리 명령이 실패한 실제 작업이 정리 후보에 "스쿼시·PR로 닫힘 — 브랜치의 개별 커밋은 기준 브랜치에 없습니다"로 나오고, 원인을 고친 뒤 정리하면 브랜치와 폴더가 지워진다. 작업의 정리 알림은 "설정 → 에이전트 → 정리 후보" 길을 안내한다 | Fix D 1차 |
| CL3 | 초기화된 하위 모듈이 있는 프로젝트의 작업 폴더: 정리는 `agents.worktree_has_submodules` 문구로 거절하고, 그 문구의 `git worktree remove --force '<폴더>'`가 프로젝트 폴더의 터미널에서 실제로 되는지 확인한다. 버리기 확인 창은 "하위 모듈이 있습니다…" 줄을 보인다 | Fix D 2·3차 |
| CL4 | S7-14 중 만난 오류 화면이 제목만 보이고 다음 행동이 빠졌는지 기록한다(알려진 곳: 이미지 붙이기, 노트 읽기 전용 줄, 활동 하루·주간, 파일 작업 대화상자, 파일 트리, 소스 페이지, 빠른 열기, 에이전트 실행 탭의 `cancelled`·`not_found` 줄). 고치지는 않는다 | Fix D |
| CL5 | 새 작업 창의 브랜치 이름 칸에 대문자를 입력하면 소문자로 바뀌어도 커서 위치가 그대로다(WebView2. 지금은 jsdom 시험뿐이다) | Fix D |
| CL6 | 에이전트 상세의 [터미널 보기]와 재개가 터미널에 포커스를 준다. 손으로 고른 터미널 탭은 포커스를 가져가지 않는다 | Fix D |
| S6 묶음 | G1·G2·A1–A6을 같은 harness로 다시 돈다. A3·A5는 검토된 helper가 없으면 NOT_RUN이다. A6은 uninstall 등록 key를 30초까지 polling하도록 harness만 고친다 | B7 |
| N1 | accept 설치본의 설치 폴더에 `LICENSE`와 `THIRD_PARTY_NOTICES.md`가 있고, 고지 파일에 D2Coding의 NAVER 저작권 줄과 OFL 본문이 든다. prod 쪽은 prod를 설치하지 않고 확인한다: 후보 SHA의 `windows.yml` 실행 로그에서 prod 채널의 `win-package.ps1 -Verify`(빌드 결과 폴더에 두 파일이 있고 저장소의 파일과 해시가 같은지 본다)가 통과했는지 보거나, 받은 prod 설치 파일(`devbox-setup` 산출물)의 내용 목록에서 두 파일을 찾는다. 설치된 prod 폴더는 사용자가 S7-16에서 확인한다(prod는 시험으로 바꾸지 않는다). 둘 다 못 하면 prod 쪽만 NOT_RUN으로 적고 이유를 쓴다 | S7-12, Fix E 8 |
| SV1 | accept 인스턴스에서 프로젝트 `[[service]]`를 시작하고, 로그가 보이는지 확인하고, 멈춘다. 데몬 RPC로 확인한다 | 13개 중 3 |
| NT1 | `devbox note add`로 accept 인스턴스에 노트를 쓰고 검색으로 찾는다 | 13개 중 4 |
| DS1 | `wsl.exe -l -q`로 배포판 수를 센다. 내부 배포판이 아닌 것이 2개 이상이면 선택 화면을 확인하고, 하나뿐이면 해당 없음 | 13개 중 13 |
| R | SC2 cold raw를 다시 잰다(rc.1 값과 비교). SmartScreen 문구를 관찰한다 | SC2, 문서 |

**07c 13개 확인의 담당**

| 번호 | 확인 | 담당 |
|---|---|---|
| 1 | 새 설치 첫 화면 | S7-14 S6 묶음(새 설치) |
| 2 | agent 전 과정 | S7-14 H1–H4 |
| 3 | service/log | S7-14 SV1 |
| 4 | note 작성 | S7-14 NT1 |
| 5 | v0.9 제거 후 정상 | 사용자(S7-16 인계) |
| 6–7 | 배율 150%/200% 가독성, 키보드만으로 palette→작업→확인→오류 재시도 | 사용자 |
| 8–9 | API 응답 일일 기록 append, editor JSON 선택→변환 | 사용자. 검토된 화면 조작 helper가 없다 |
| 10–11 | R1 30분 유휴 유지, R2 WSL shutdown 뒤 재연결과 걸린 시간 | 사용자 |
| 12 | 방해 금지 알림 센터 기록, 해제 후 popup/taskbar | 사용자 |
| 13 | 배포판 2개 이상 선택 화면 | S7-14 DS1 |

사용자 담당 항목은 S8-5 승인 때 결과가 없어도 된다(07c). 그때는 NOT_RUN으로 적는다.

**마무리**
- [x] 4. prod 경로가 그대로인지 S6과 같은 방식으로 확인한다. 소유 자원만 정리한다.
- [x] 5. 결과를 PROGRESS 작업 사본과 PR 본문에 남긴다. 결과만 담은 문서 커밋은 제품 입력을 바꾸지 않는다(CONVENTIONS §8).

## S7-15 PR·리뷰·머지

- [x] 1. 최종 Biome를 돌리고 push한 뒤 `gh pr create --base main`으로 PR을 연다. 본문에 담을 것:
  - 범위와 결정 1–6
  - 선택 과제 답
  - 시나리오 결과, NOT_RUN과 그 이유
  - 리뷰 상태
- [x] 2. CONVENTIONS §8 새 규칙을 적용한다.
  - 10분 동안 봇 반응을 본다(켜짐·꺼짐 판정).
  - 켜짐이면 최대 3회 반영한다.
  - 꺼짐이면 독립 코드·보안 검토를 1회만 한다.
- [x] 3. 최종 head에서 `Frontend (pnpm)`, `Rust (Cargo workspace)`, `Rust (Windows)`가 SUCCESS여야 한다. e2e 등 실제 요구 job도 확인한다. 보호 규칙은 유지한다.
- [x] 4. squash 머지 뒤 할 것:
  - main CI 확인.
  - 원본 checkout `git pull --ff-only`.
  - 검증 tree와 머지 tree가 같은지 대조.
  - worktree 정리: worktree 제거 → prune → 로컬·원격 `v1/s7-rc2`·`v1/s7-rc1-accept` 삭제.

## S7-16 rc.2 prerelease — 별도 승인

- [x] 1. 태그 메시지(릴리스 본문) 초안을 쓴다.
  - rc.1 이후 바뀐 것: B1–B4, I1, 문서, 라이선스.
  - 알려진 제한과 NOT_RUN 목록.
  - RC는 업데이트가 꺼져 있어 직접 설치해야 한다는 점.
- [x] 2. 사용자에게 묻는다: "main `<sha>`에 `v1.0.0-rc.2` 주석 태그를 push해 prerelease를 공개할까요?" 답이 "예"일 때만 진행한다.
  1. `git tag -a v1.0.0-rc.2 <sha> -F <노트 파일>`
  2. `git push origin v1.0.0-rc.2`
  3. release run SUCCESS, asset SHA-256을 확인한다. `releases/latest`는 여전히 v0.9.0이어야 한다.
- [x] 3. 사용자 인계. 각 항목을 PROGRESS에 기록한다.
  - prod rc.2 설치는 사용자가 직접 한다.
  - S7-14 담당 표에서 사용자 몫(13개 중 5–12)을 부탁한다. R1(30분 유휴 유지)과 R2(`wsl --shutdown` 뒤 재연결과 걸린 시간)가 여기에 들어 있다.

---

## S8 — 1.0.0

S7 머지와 rc.2 공개 뒤, `v1/s8-release`를 origin/main에서 만든다. 제품 코드는 버전만 바꾼다.

### S8-0 착수

- [x] PROGRESS에 S7 결과를 기록한다(S7 작업 사본에서 옮김): rc.2 공개, 사용자 확인 결과.

### S8-1 버전 `1.0.0` (B5)

- [x] S7-13 2단계와 같은 방식으로 한다.
- [x] 시험 `update::service::stable_version_enables_updates`: `1.0.0`이고 prod면 업데이트가 켜진다(`app/src-tauri/src/core/update/service.rs:57` 경로). 같은 뜻의 시험이 이미 있으면 그것을 확인만 한다.
- [x] 커밋: `chore(release): set version to 1.0.0`.

### S8-2 문서 통폐합 (결정 4)

**보관**
- 대상: `00, 02, 03, 03b, 04, 04b, 05, 06, 07, 07a, 07b, 08, 09`와 `07c`. 07c는 S6-6c에 남은 확인 목록을 PROGRESS §3으로 옮긴 뒤 보관한다.
- 트리에서 지운다. `docs/history.md`를 만들어 S7 머지 SHA의 고정 링크와 `git show <sha>:<경로>`를 둔다. v0.9 기록(`72ff50c7`)도 같은 곳에 둔다.
- 이 문서(10)는 1.0.0 공개 기록 뒤 S8-6에서 보관한다.

**참조 수정**
- README :5·:7-8, AGENTS :6, CONVENTIONS :4-6·:27
- ADR 0017 :11(09 링크)
- `app/acceptance/README.md` :9·:31

**PROGRESS 재구성** — 같은 파일을 200줄 이하로 줄인다. 새 파일을 만들지 않는다(CONVENTIONS §11).
- 구성:
  - 0 읽는 법
  - 1 현재 위치
  - 2 열린 결정
  - 3 1.0.0 공개 게이트(13개, I1, SC1–10을 한 줄씩 PASS/FAIL/NOT_RUN)
  - 4 열린 후속(영역별 한 줄)
  - 5 유효한 결정(링크)
  - 6 근거 보관(이 PC 전용이라고 명시)
  - 7 최근 세션 10줄
- 이전 전문은 S7 머지 SHA의 고정 링크로 남긴다.
- "오래된 줄은 지우지 않는다" 규칙(PROGRESS :1561)을 바꾸는 근거는 결정 4다.

**CONVENTIONS**
- 1.0 이후 작업 모델: 작업 하나 = `feat|fix|docs|release/*` 브랜치 하나 = PR 하나. 모든 릴리스는 main에서 한다.
- 릴리스 노트 = 주석 태그 메시지.
- 버전 파일 4곳 + lock 2곳.
- S 단위 문구를 지운다.

**새 문서와 합치기**
- `docs/guide.md` 만들기: 화면별 흐름, 단축키, CLI(`devbox doctor`, `devbox note add [--daily]`, `devbox secrets export-key|import-key`).
- `app/README.md`가 README의 개발 절을 흡수한다. README에는 개발 3줄과 링크, "문서" 절(guide, config, SECURITY, history, CONVENTIONS)만 남긴다.

**ADR**
- 0006·0008·0013: 상태를 "대체됨(0017)"으로 바꾸고 이유를 한 줄 적는다.
- 0005·0016: 0017 결과에 v1 형태를 한 줄씩 적는다.
- 0017: 제목 번호, "초안" 문구, 09 고정 링크.
- `adr/README.md` 표를 맞춘다.

**ui-terms와 01-design**
- `ui-terms`: "데몬" 정의를 실제 사용에 맞춰 "연결 상태 표시·오류 안내 포함"으로 넓힌다. S8은 제품 문구를 바꾸지 않는다.
- `01-design`: 00 §7 추적표를 §9.4로 흡수한다. §7 설정 표는 `docs/config.md` 링크로 바꾼다.

**검증과 커밋**
- 깨진 상대 링크 0건을 확인한다. 검사 방법은 감사 때와 같고, 인라인 코드는 제외한다.
- 커밋을 넷으로 나눈다:
  - `docs: archive completed v1 plans behind a history index`
  - `docs: restructure PROGRESS around the 1.0.0 gate`
  - `docs: add the user guide and merge development docs`
  - `docs: correct ADR status after the v1 rebuild`

### S8-3 검사·후보

- [x] `pnpm check`를 한 번 돌린다. 버전이 바뀌었기 때문이다.
- [ ] 1.0.0 후보로 accept 설치 스모크를 한다: 첫 화면, 버전 표시 1.0.0, H1 하나, N1 (S8-4b squash SHA로).
- [x] 1.0.0 릴리스 노트(태그 메시지) 초안을 쓴다. 담을 것: 새 구조, v0.9.0 수동 제거 순서, RC→1.0 수동 설치, 알려진 제한·NOT_RUN.

### S8-4 PR·리뷰·머지

S7-15와 같다.

### S8-4b 최종 교차 검토 수정 — 사용자 결정 2026-11-06

- **입력:** S8-4 머지 뒤 출시 전 최종 교차 검토의 통합 보고(Git common-dir `devbox-verification/s8/final-review/final-report.md`).
  - 검토자 5명: Claude Opus 5.5 max 2명, Codex GPT-6-Astra max 3명.
  - 방식: 1차는 영역별 독립 검토, 2차는 다른 계열이 교차 검증. 컨트롤러가 Critical·Important를 코드로 확인했다.
  - 결과: 52건 → Critical 0, Important 21, Minor 31.
- **사용자 결정(원문, AskUserQuestion):**
  - 범위: "11건+문서 고친 뒤 공개 (권장)" — 통합 보고 §3의 A 11건과 §5의 문서. §4 B 7건은 넣지 않는다.
  - CA-01: "자동 시작 유지, 문서만" — 언어 서버의 자동 시작을 바꾸지 않는다. 위험은 SECURITY.md·README에 적는다(S8-4b-11).
  - 계획 승인(원문): "한글 제목 변경(9)은 빼고 진행" — S8-4b-9(CB-02)는 하지 않고 1.0.x 후속으로 옮긴다. 한글만 있는 제목은 지금처럼 브랜치 이름을 직접 입력해야 한다.
  - 그래서 코드 과제는 9개다.
- **작업 위치:** `fix/v1-final-review`, `/home/jihoon/projects/devbox-wt/fix-v1-final-review`. origin/main `08dffcd3`에서 갈라진다.
  - main을 고정하던 Ruling 123은 이 PR로 끝난다.
  - 1.0.0 태그 후보는 이 PR의 squash SHA다. S8-3의 accept 스모크도 그 SHA로 한다.
- **범위 밖(1.0.x):** 통합 보고 §4 B와 §6 D. S8-4b-11에서 PROGRESS §4로 옮긴다. 범위를 더하거나 빼야 하면 멈추고 묻는다.
- **공통 규칙:** §3 그대로다.
  - `PROTOCOL_VERSION`은 1로 둔다.
  - RPC 타입이 바뀌면 `pnpm gen`을 돌린다.
  - 고지 입력(lock·manifest)이 바뀌면 같은 커밋에서 `cargo run -p xtask -- notices`를 돌린다.
- **시험 실행:** 시험 이름은 필터 문자열이다. Rust는 `cargo test -p <crate> [--test <file>] <이름> -- --test-threads=2`, 화면은 `pnpm --filter app exec vitest run <path>`이고, 둘 다 `scripts/verify-resources.py` 안에서 돌린다.

**검토 초점(S8-4b).** 시험이 놓치기 쉬운 실제 조건이다.

1. **비밀 값의 이스케이프 표기.** 비밀 값에 따옴표·역슬래시·줄바꿈이 있고, 실행한 프로그램이 그 값을 JSON이나 Rust Debug로 찍는다. 기대: 그 표기가 실행 로그 파일·`runtime.logs`·MCP `devbox_logs` 어디에도 남지 않는다. — S8-4b-1
2. **프로젝트 밖을 가리키는 진단.** Rust 저장 검사의 진단이 `~/.cargo/registry` 안의 관련 위치를 가리킨다. 기대: 세션이 끊기지 않고, 프로젝트 안 진단은 그대로 온다. — S8-4b-2
3. **공유 의존 서비스.** db(고정 포트)에 기대는 api·worker가 있고, api를 [재시작]한다. 기대: db 실행이 하나로 남고 재시작이 성공한다. — S8-4b-3
4. **긴 업데이트 설치.** 설치가 15초보다 오래 걸리고, 다른 WSL 창이 없다. 기대: 배포판이 멈추지 않는다. 실제 Windows 확인은 S8-5에서 한다. — S8-4b-10
5. **처음 쓰는 사용자.** 프로젝트가 0개다. 기대: 빈 화면에서 바로 프로젝트를 추가하고 첫 작업까지 간다. — S8-4b-8

**순서**(과제당 커밋 하나, 차례로)

| 과제 | 내용 | 커밋 |
|---|---|---|
| S8-4b-0 | 이 절 채택 | `docs: plan the 1.0.0 final review fixes` |
| S8-4b-1 | X1-01 비밀 이스케이프 표기 가림 | `fix(cli): mask JSON and debug escaped secret values in run output` |
| S8-4b-2 | CB-2-01 LSP 알림의 밖 경로 | `fix(lsp): keep the session when a server message points outside the project` |
| S8-4b-3 | X1-02 의존 서비스 재사용 | `fix(runtime): reuse a running dependency service instead of starting another` |
| S8-4b-4 | CA-02 감시 실패로 데몬이 멈추지 않게 | `fix(runtime): start the daemon when execution config cannot be watched` |
| S8-4b-5 | X2-02 Codex 재개 | `fix(agents): resume a Codex task again without a new turn` |
| S8-4b-6 | X3-01 종료 전 API 변경 확인 | `fix(app): ask before quitting with unsaved API changes` |
| S8-4b-7 | X3-05 환경 변수 사용 체크 저장 | `fix(app): keep disabled API environment variables disabled after saving` |
| S8-4b-8 | CB-01 빈 화면의 [프로젝트 추가] | `fix(app): offer adding a project from empty screens` |
| S8-4b-9 | CB-02 한글 제목 — **제외**(계획 승인 2026-11-06) | — |
| S8-4b-10 | X3-02 업데이트 중 WSL 유지 | `fix(app): keep WSL running while an update installs` |
| S8-4b-11 | 문서 | `docs: document final review limits, language server risk and Codex resume` |
| S8-4b-12 | 브랜치 전체 검토·전체 검사 | 고칠 것이 있으면 그 수정 커밋 |
| S8-4b-13 | push·PR·리뷰·머지 | — (S8-4와 같다) |

#### S8-4b-0 채택

- [x] 이 절을 계획 10에 넣고 커밋한다. 다른 파일은 바꾸지 않는다. 링크 검사와 Biome을 돌린다.

#### S8-4b-1 X1-01 — 비밀 값의 JSON·Debug 표기도 가린다

**문제.** 래퍼의 가림(`crates/cli/src/exec/secrets.rs:79-95` `Secrets::redactor`)은 원문 바이트만 찾는다. 그래서 따옴표·역슬래시·줄바꿈·제어 문자가 든 비밀을 프로그램이 JSON이나 Rust Debug로 찍으면, 그 표기가 실행 로그와 MCP `devbox_logs`에 남는다. 지원 번들은 이미 같은 표기를 가린다(`crates/tools/src/support/redact.rs:24-44`).

**파일**
- 수정: `crates/cli/src/exec/secrets.rs`(`redactor`). 시험은 같은 파일의 시험 모듈에 둔다.
- 통합 시험: `crates/cli/tests/runtime_secrets.rs`.

**동작**
- 패턴은 값(`values`)과 mask(`masks`)마다 원문을 넣는다.
- 그 바이트가 UTF-8이면 두 표기를 더 넣는다. 원문과 같으면 넣지 않는다.
  - `serde_json::to_string(text)`의 바깥 따옴표를 뺀 안쪽
  - `format!("{text:?}")`의 바깥 따옴표를 뺀 안쪽
- 표기는 지원 번들과 같다. 새 패턴 버퍼는 `Zeroizing`이고, 함수 시그니처는 그대로다.
- 이미 저장된 로그는 다시 가리지 않는다(S8-4b-11의 SECURITY.md 문장).

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `json_escaped_secret_is_masked_at_every_split`
    - 비밀 원문: `ow"n\ed` + 줄바꿈 + `x` + U+0001.
    - 입력: JSON 표기를 담은 `{"t":"<JSON 안쪽>"}`.
    - 기대: 모든 분할 위치에서 출력이 `{"t":"[REDACTED]"}`다.
  - `debug_escaped_secret_is_masked_at_every_split`
    - 같은 비밀의 Debug 표기 `t=Some("<Debug 안쪽>")`.
    - 기대: 출력이 `t=Some("[REDACTED]")`다. U+0001이 JSON은 `\u0001`, Debug는 `\u{1}`이라 두 표기가 다르다.
  - `run_output_hides_json_escaped_secret`(통합)
    - 따옴표가 든 비밀을 쓰는 작업이 `printf`로 JSON 이스케이프 표기를 찍는다.
    - 기대: 그 표기가 로그 파일과 `runtime.logs` 응답에 없고, `[REDACTED]`가 있다.
    - MCP `devbox_logs`도 같은 로그를 읽는다. `crates/cli/tests/mcp_runs.rs`의 helper로 함께 확인할 수 있으면 같은 시험에 넣는다.
- [x] 2. 시험이 실패하는 것을 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `crates/cli/src/exec/secrets.rs` 시험 모듈, `--test runtime_secrets`가 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-2 CB-2-01 — 서버 메시지의 밖 경로로 LSP 세션을 끊지 않는다

**문제.** 서버 알림이나 서버 요청 안의 `uri`가 프로젝트 밖이면, 다음 순서로 세션이 멈춘다.
1. `Relay::server`가 `Err`를 돌려준다(`crates/lsp/src/relay.rs:327-331`).
2. actor가 세션을 실패로 보고 서버를 다시 띄운다(`actor.rs:406-411`).
3. 5분 안에 세 번이면 "실행 실패"로 고정된다.

응답은 같은 경우에 그 요청만 거절한다(`relay.rs:285-292`, 시험 있음).

**파일**
- 수정: `crates/lsp/src/relay.rs`(`server`의 method 분기).
- 시험: `crates/lsp/src/relay/tests.rs`.

**동작**
- `textDocument/publishDiagnostics`
  - `params.uri`가 밖이면 그 알림만 버린다.
  - 안이면, 각 진단의 `relatedInformation` 중 `location.uri`가 밖인 항목만 뺀다. 나머지는 지금처럼 정규화·버전 확인·전달·문제 패널 처리를 한다.
- id가 있는 서버 요청(예: `window/showDocument`)에 밖 경로가 있을 때
  - 클라이언트에 보내지 않는다.
  - 서버에 `{"jsonrpc":"2.0","id":<같은 id>,"error":{"code":-32602,"message":"Outside project"}}`로 답한다.
  - `workspace/applyEdit`의 기존 `applied:false` 처리는 그대로다.
- 그 밖의 알림에 밖 경로가 있으면 그 알림만 버린다. 기록은 `tracing::debug!`로 하고 경로는 쓰지 않는다.
- 위 경우는 모두 `Ok`로 끝나 세션이 유지된다. 깨진 JSON 같은 `LspError::Protocol`은 지금처럼 실패다.

**단계**
- [x] 1. 실패 시험을 쓴다. 기존 시험의 밖 경로 `file:///dependency/lib.rs`를 쓴다.
  - `diagnostics_keep_the_session_and_drop_only_outside_related_locations`
    - 열린 문서의 진단에 관련 위치가 둘 있다(안 하나, 밖 하나).
    - 기대: 클라이언트 메시지에 안쪽 하나만 남고, `ready()`가 true다.
  - `diagnostics_for_an_outside_document_are_dropped_and_the_session_stays_ready`
  - `server_request_with_an_outside_uri_gets_an_error_reply_and_is_not_forwarded`
    - 기대: `server`에 같은 id의 오류 응답이 있고, `clients`는 비어 있다.
  - `notification_with_an_outside_uri_is_dropped_and_the_session_stays_ready`
- [x] 2. 실패를 본다. 지금은 `Err`가 난다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 기존 "ordinary outside target refusal preserves shared generation" 시험, `cargo test -p devbox-lsp`가 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-3 X1-02 — 실행 중인 의존 서비스를 다시 띄우지 않는다

**문제.** `start_graph`(`crates/runtime/src/execution.rs:281-289`)는 마지막 노드만 기존 실행을 대체한다. 의존하는 서비스는 실행 중이어도 새 UUID로 다시 띄운다. 그래서 고정 포트 db에 기대는 api를 시작하거나 [재시작]하면 db 단계에서 실패한다. 설계 01 :112·:836은 "앞 서비스의 준비됨을 기다린다"고 정했다.

**파일**
- 수정: `crates/runtime/src/execution.rs`. 필요하면 `crates/runtime/src/run_store.rs`에 조회 함수를 더한다.
- 시험: `crates/cli/tests/runtime_lifecycle.rs`.
- (구현 반영) 재사용 판단은 새 모듈 `crates/runtime/src/dependency.rs`에 두었다. systemd 시험은 `crates/cli/tests/runtime_lifecycle_cases/dependencies.rs`(`#[ignore]`, `runtime_lifecycle.rs`가 부른다)에, CI에서 도는 시험은 `crates/runtime/src/dependency_tests.rs`에 있다.

**동작**
- 그래프에서 마지막이 아닌 서비스 노드는, 같은 프로젝트·worktree·정의 id의 실행 중 서비스(`state.is_live()`)가 있으면 새로 띄우지 않는다.
  - 대신 그 실행의 준비됨을 기다린다(`wait_ready`, 기존 60초).
  - (구현 반영) 살아 있지만 준비 시한 안에 준비되지 않으면 `wait_ready`의 `runtime.not_ready`이고(이 과제 전과 같다), 더 이상 살아 있지 않을 때만 `runtime.dependency_failed`다.
  - (구현 반영) 재사용하기 전에 systemd로 그 실행이 아직 살아 있는지 맞춰 보고(`reconcile_one`, 사라졌으면 새로 띄운다), 후보가 여럿이면 준비된 실행을 먼저 고른다(`run_store::live_services`).
  - 재사용한 실행은 이 시작이 띄운 실행 목록(`started`)에 넣지 않는다. 호출자는 이 목록을 시작 결과 보고에만 쓴다(`crates/cli/src/daemon/launch.rs:158-167`의 서비스 수·run id, `crates/runtime/src/scheduler.rs:30-39`의 실패 보고).
  - 재사용할 실행이 예전 정의로 떠 있어도 그대로 재사용한다. 설계 01 :552 "설정이 바뀌면 실행 중인 것은 그대로 둔다"와 같다.
- 마지막 노드(사용자가 고른 정의)는 지금 그대로다. [시작]은 새 실행, [재시작]은 그 실행의 대체다.
- 작업(task) 의존은 일회성이라 지금처럼 매번 실행한다.
- 같은 정의 식별(프로젝트·worktree·정의 id)의 서비스 시작은 직렬화한다. 동시에 두 번 시작해도 하나만 뜬다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - db: 시험이 실행 중에 고른 빈 포트를 `ports_hint`로 쓰는 서비스다. 잘 알려진 포트는 쓰지 않는다(기존 서비스를 건드리지 않는다).
  - api·worker: `depends_on = ["db"]`인 서비스다.
  - `restarting_a_service_keeps_its_running_dependency`
    - api 시작 → 실행 2개 → api 재시작.
    - 기대: 성공하고, db의 run id·unit이 그대로이며, db 실행은 하나다.
  - `starting_a_service_reuses_a_dependency_started_on_its_own`
    - db 시작 → api 시작.
    - 기대: db 실행이 하나다.
  - `two_services_share_one_running_dependency`
    - api 시작 → worker 시작.
    - 기대: db 실행이 하나다.
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `depends_on_runs_in_order`·`depends_on_stops_after_failure`·`manual_restart_resets_the_start_limit`, `cargo test -p devbox-runtime`이 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-4 CA-02 — 실행 정의 감시가 실패해도 데몬은 뜬다

**문제.** 감시기 생성 실패나 디렉터리 `watch` 실패가 `?`로 올라간다(`crates/runtime/src/watch.rs:60`·`:172-174`). 그러면 `serve()`가 접속을 받기 전에 끝나고(`crates/cli/src/daemon/server.rs:72-74`), systemd가 재시작을 되풀이한다. 원인은 inotify 한도 소진(설계 R14)이나 읽을 수 없는 디렉터리다. 같은 데몬의 문서·문제 감시기는 이런 실패를 경고만 한다.

**파일**
- 수정: `crates/runtime/src/watch.rs`.
- 시험: `crates/cli/tests/daemon.rs`(또는 runtime의 기존 감시 시험 모듈).
- (구현 반영) 시험은 `crates/cli/tests/runtime_definitions.rs`에 두었다. 정의 읽기도 고쳤다: `crates/projects/src/config/copy.rs`가 중간 폴더를 `O_PATH`로 연다(아래 "실행 비트만" 전제가 틀렸다).

**동작**
- 디렉터리별 `watch` 실패
  - 그 디렉터리만 감시 목록에서 빼고 경고를 남긴다. 경로 대신 프로젝트 id를 쓴다.
  - 정의는 지금처럼 읽는다.
  - 다음 refresh(`projects.changed`, 기존 주기 갱신)에서 다시 감시를 붙인다.
  - (구현 반영) 주기 갱신은 없었다. 그래서 감시가 하나라도 빠져 있는 동안 30초마다 refresh를 다시 돌린다(정의도 다시 읽는다).
- 감시기 생성 실패
  - 감시 없이 정의를 한 번 읽어 쓴다.
  - 30초마다 생성을 다시 시도하고, 붙으면 지금처럼 동작한다.
- 정의 읽기와 DB의 실패는 지금 처리 그대로다. 감시와 관련된 실패만 시작을 막지 않는다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `daemon_starts_when_a_project_config_directory_cannot_be_watched`
    - 프로젝트 A를 담은 상위 폴더를 `chmod 111`(실행 비트만)로 만든다.
      - 감시 대상은 프로젝트의 상위 폴더·root·`.devbox`다(`crates/runtime/src/watch.rs:146-158`).
      - 실행 비트만 있으면 `is_dir()`와 정의 읽기는 되지만 `inotify_add_watch`는 EACCES다.
      - (구현 반영) 이 전제는 틀렸다. 중간 폴더를 `O_RDONLY`로 열어 정의 읽기도 EACCES였고, 위 `copy.rs` 수정 뒤에 읽힌다.
      - 시험이 실패해도 권한을 되돌리는 guard를 둔다. root로 돌면 이 시험은 건너뛴다.
    - 시험 데몬을 띄운다.
    - 기대: `system.ping`이 성공하고, 다른 프로젝트 B의 실행 정의 목록이 정상이다.
    - 권한을 되돌리고 프로젝트를 하나 더 등록해 `projects.changed`를 일으킨다.
    - 기대: A의 `.devbox/devbox.toml` 변경이 목록에 반영된다.
- [x] 2. 실패를 본다. 지금은 데몬이 뜨지 않는다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `cargo test -p devbox-runtime`, `--test daemon`이 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-5 X2-02 — 새 턴 없이 끝난 Codex 작업도 다시 재개한다

**문제.**
- Codex 재개는 `refresh_binding(&task, true)`를 거친다(`crates/agents/src/resume.rs:99`).
- 첫 재개가 binding의 `launch_ms`를 바꾸고 `turn_id`를 비운다(`:177-179`).
- tracker는 그 launch 뒤의 턴만 인정한다.
- 그래서 재개한 뒤 새 지시 없이 끝낸 작업은 다음 재개가 `agents.session_metadata`로 막힌다.

**파일**
- 수정: `crates/agents/src/resume.rs`, `crates/agents/src/lifecycle.rs`, `crates/agents/src/tracker.rs`.
- 시험: 기존 재개 시험 파일(`crates/agents/src/resume_*tests.rs`)과 tracker 시험.

**동작**
- 재개 때는 "이 작업의 세션이 무엇인가"만 확인한다.
  - 저장된 binding에 전에 검증한 thread id가 있으면 그 id를 쓴다. 단 그 rollout 파일이 있고 세션 메타데이터(`source=cli`, 작업의 workdir)가 맞아야 한다. 마지막 launch 뒤의 턴은 요구하지 않는다.
  - (구현 반영) 저장된 thread로 재개할 때는 rollout의 첫 `session_meta` 줄만 한 번 확인하고(재시도 없음) binding을 DB에 다시 쓰지 않는다(`tracker::session`).
  - 저장된 id가 없으면 지금처럼 `refresh_binding`으로 찾는다. 못 찾으면 `agents.session_metadata`다.
- 턴 상관 검증(완료 보고·idle 판정)은 지금 그대로다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `codex_task_resumed_without_a_new_turn_resumes_again_with_the_same_thread`
    - fixture rollout(턴 1개)로 시작한다 → 재개 1회(새 턴 없음) → 도구 종료 → 재개 2회.
    - 기대: 두 번째 시작 스크립트에 `resume <같은 thread id>`가 있다.
  - `codex_resume_refuses_a_stored_thread_whose_rollout_names_another_workdir`
    - 기대: `agents.session_metadata`.
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 기존 재개 rollback 시험, `cargo test -p devbox-agents`가 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-6 X3-01 — 종료·업데이트 전에 저장하지 않은 API 변경을 묻는다

**문제.**
- 종료 준비(`app/src/shell/NativeQuitPreparation.tsx:21`)는 문서 저장소만 확인한다.
- API 요청 탭(`app/src/features/tools/api/store.ts:115`)과 환경 편집 초안(`EnvEditor.tsx`의 `useEnvDrafts`)은 메모리 전용이다.
- 그래서 트레이 [종료]와 업데이트 [설치하고 다시 시작]이 묻지 않고 버린다.
- 업데이트의 종료 준비는 설치된 판의 화면이 하므로, 1.0.0에 넣어야 첫 업데이트를 보호한다.

**파일**
- 수정: `app/src/shell/NativeQuitPreparation.tsx`, `app/src/shell/quitPreparation.ts`.
- 수정: `app/src/features/tools/api/store.ts`(저장하지 않은 요청 수), `EnvEditor.tsx`(저장하지 않은 환경 초안 수를 내보내는 함수).
- 새 파일: `app/src/features/tools/api/quitGuard.tsx`(확인 창).
- 시험: `app/src/shell/quitPreparation.test.ts`, 새 `quitGuard.test.tsx`.

**동작**
- main 창의 종료 준비는 문서 확인 전에 API 변경을 본다. 저장하지 않은 요청 탭(`isDirty`)과 환경 초안이 대상이다.
- 하나라도 있으면 확인 창을 띄운다.
  - 제목: "저장하지 않은 API 변경이 있습니다"
  - 본문: "요청 {n}개와 환경 {m}개의 변경을 저장하지 않았습니다. 종료하면 이 변경은 사라집니다." 0개인 쪽은 빼고 쓴다. 예: "요청 2개의 변경을…".
  - 버튼: [변경 버리고 종료]·[취소]
- [취소]
  - 종료 준비가 `safe=false`로 끝나 종료를 취소한다. 업데이트도 설치하지 않는다.
  - 기존 차단 안내(`blocked`)는 띄우지 않는다.
  - (구현 반영, Ruling 136) main 창은 `blocked`를 띄우지 않지만, native ACK에 거절 이유가 없어 트레이는 "종료하지 못했습니다…", 업데이트는 `quitRejected` 문구, 열린 팝아웃은 일반 `blocked()` 안내를 보인다. 1.0.x 후속이고, 1.0.0에서 처음 업데이트할 때도 이 문구가 나온다.
- [변경 버리고 종료]: 문서 확인으로 넘어간다.
- 저장 버튼은 두지 않는다. 새 요청은 파일 경로가 없을 수 있다.
- 초안을 디스크에 자동 저장하지 않는다. 비밀이 있을 수 있다.
- 노트·코드 팝아웃 창은 API 탭이 없으므로 지금 그대로다.
- 확인 창은 주 창에 띄운다. 트레이 [종료]는 확인 전에 주 창을 앞으로 가져오고(`app/src-tauri/src/platform/tray.rs:68-70`), 업데이트는 주 창 안의 확인 창에서 시작한다.
- 종료 준비의 30초 시한(`app/src-tauri/src/platform/quit.rs:166`)은 문서 확인과 똑같이 적용된다. 넘기면 종료를 취소한다(안전 쪽). 시한은 바꾸지 않는다.
- [변경 버리고 종료]는 API 저장소를 비우지 않는다. 이어지는 문서 확인에서 취소하면 초안이 그대로 남는다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `api_changes_cancelled_quit_acks_unsafe_without_asking_documents`
  - `api_changes_discarded_continue_to_documents`
  - `no_api_changes_go_straight_to_documents`
  - `api_discard_keeps_drafts_when_documents_cancel`
  - `quit_guard_names_only_the_kinds_with_changes`
  - `quit_guard_cancel_resolves_false_and_discard_resolves_true`
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `app/src/shell`·`app/src/features/tools/api` 시험이 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-7 X3-05 — 끈 환경 변수는 저장 뒤에도 꺼져 있다

**문제.**
- 환경 파일 형식(`app/src/features/tools/api/model/envFile.ts:10-14`)에 사용 여부가 없다.
- 저장(`EnvEditor.tsx:173`)은 모든 행을 켠 것으로 쓴다.
- 그래서 다시 열면 끈 행이 켜진다. 같은 이름의 행이 있으면 꺼 둔 값(예: 운영 주소)으로 요청이 나갈 수 있다.

**파일**
- 수정: `envFile.ts`, `EnvEditor.tsx`.
- 시험: `envFile`·`EnvEditor` 시험 파일.

**동작**
- `EnvVariable`에 선택 필드 `enabled?: false`를 둔다.
  - 쓸 때는 끈 행에만 `"enabled": false`를 적는다.
  - 읽을 때 없으면 켬이다. `enabled`가 있는데 `false`가 아니면 형식 오류다.
- 편집기
  - 읽을 때 `enabled: variable.enabled !== false`로 둔다.
  - 저장할 때 행의 체크를 옮긴다.
  - 활성 환경(`pairs`)은 지금처럼 켠 행만이다.
- 바뀌는 점
  - 끈 행이 있는 환경 파일에는 `"enabled": false`가 더해진다. 프로젝트 컬렉션에 커밋된 환경 파일이라면 diff가 생긴다.
  - 이 판보다 앞선 판은 이 필드를 무시해 끈 행을 켠 것으로 읽는다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `disabled_variable_round_trips_as_enabled_false`
  - `enabled_other_than_false_is_rejected`
  - `files_without_enabled_stay_enabled`
  - `unchecked_row_stays_unchecked_after_save_and_reopen`
    - 같은 이름 두 행 중 하나를 끄고 저장한 뒤 다시 연다.
    - 기대: 그 행은 꺼져 있고, 활성 환경 값은 켠 행의 값이다.
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `app/src/features/tools/api` 시험이 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-8 CB-01 — 프로젝트가 없는 빈 화면에 [프로젝트 추가]

**문제.**
- 프로젝트가 0개일 때 빈 화면과 새 작업 대화상자는 안내만 있고 동작이 없다.
- [프로젝트 추가]는 제목 표시줄 전환기(`ProjectSwitcher.tsx:284`)에서만 열린다.
- 설계 01 :637은 빈 상태에 주 동작 하나를 둔다고 정했다.

**파일**
- 새 파일: `app/src/features/projects/AddProjectAction.tsx`. 버튼과 `AddProjectDialog`를 묶는다.
- 쓰는 곳 6곳:
  - `app/src/features/agents/AgentList.tsx:50`
  - `app/src/features/agents/NewTaskDialog.tsx:179`
  - `app/src/features/runs/RunsPage.tsx:46`
  - `app/src/features/source/SourcePage.tsx:55`
  - `app/src/features/problems/ProblemsPanel.tsx:18`
  - `app/src/features/runs/BottomLogs.tsx:15`
- 시험: 새 `AddProjectAction.test.tsx`와 위 화면들의 시험 파일.

**동작**
- 등록된 프로젝트가 0개일 때만 [프로젝트 추가] 버튼(`variant="primary"`)을 보인다. 누르면 `AddProjectDialog`가 열린다.
  - 빈 상태: `EmptyState`의 `action`에 넣고, 제목은 "등록된 프로젝트가 없습니다"로 바꾼다. 본문은 화면별 설명 그대로다.
  - 새 작업 대화상자: "에이전트 작업을 만들 프로젝트를 등록해 주세요." 줄 옆에 버튼을 둔다. 등록이 끝나면 그 프로젝트를 고른 상태가 된다. 지금 목록 갱신으로 그렇게 되는지 시험으로 확인하고, 안 되면 그렇게 만든다.
- 프로젝트가 있으면 지금 그대로다.
- 바꾼 문구를 e2e·문서에서 찾아(`rg -n '프로젝트를 선택하세요' -g '!node_modules'`) 같은 과제에서 고친다.

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `add_project_action_opens_the_add_dialog`
  - 각 화면: `shows_add_project_when_no_project_is_registered`, `hides_add_project_when_projects_exist`
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 바꾼 화면들의 시험이 PASS.
- [x] 5. 검토 뒤 커밋한다.

#### S8-4b-9 CB-02 — 제외

- 사용자가 계획 승인에서 뺐다("한글 제목 변경(9)은 빼고 진행", 2026-11-06). 화면은 지금처럼 브랜치 이름 입력을 요구한다. S8-4b-11에서 PROGRESS §4 후속에 적는다.

#### S8-4b-10 X3-02 — 업데이트를 설치하는 동안 WSL 연결을 유지한다

**문제.**
- 업데이트 설치 동안 앱은 WSL 브리지(`wsl.exe`)를 닫는다.
- 설치는 설치된 앱의 복사본 `runner.exe`가 맡는데(`app/src-tauri/src/core/update/runner_files.rs:265-271`), 이 runner는 연결을 이어받지 않는다.
- 다른 WSL 창이 없고 `instanceIdleTimeout`이 기본(15초, 설계 01 :255)이면, 설치가 그보다 길 때 배포판이 멈춘다. 그러면 에이전트·터미널·서비스가 끝난다.
- 확인 창(`app/src/features/settings/UpdateConfirmation.tsx:143`)은 "계속 실행됩니다"라고 한다.
- runner는 설치된 판의 코드이므로, 1.0.0에 넣어야 1.0.0 → 다음 판 업데이트에 적용된다.

**파일**
- 수정: `runner_files.rs`. `Manifest`에 `keep_distro: Option<String>`을 `#[serde(default)]`로 더하고, schema는 1로 둔다.
- 수정: 인계를 만드는 곳(`app/src-tauri/src/platform/update_install.rs`·`update_runner` 준비 경로). 지금 연결한 배포판 이름을 넣는다.
- 수정: `app/src-tauri/src/platform/update_runner/execute.rs`(유지 연결). 창 없는 자식 생성이 필요하면 `native.rs`도 고친다.
- 수정: `UpdateConfirmation.tsx`(문구).
- 시험: runner·manifest 시험 모듈(`runner_files_tests.rs` 등)과 `UpdateConfirmation` 시험.

**동작**
- runner는 인계 검증 직후, 이전 앱이 끝나기를 기다리기 전에 유지 연결을 띄운다.
  - 명령: `wsl.exe -d <keep_distro> --exec sleep 900`
  - 창 없이(`CREATE_NO_WINDOW`) 띄운다.
- 끝내는 때
  - 새 앱을 띄운 뒤에는 그대로 두어 스스로 끝나게 한다(최대 15분). 그 안에 새 앱이 연결한다.
  - 설치하지 않고 끝나는 경로(취소·검증 실패·시간 초과)에서는 바로 끝낸다.
  - (구현 반영, Ruling 126·138) 바로 끝내는 것은 이전 앱이 끝난 것을 확인하기 전의 실패(취소·권한 대기 시간 초과·arm·기록·신호 실패·그 구간의 신원 오류)뿐이고, 오류 기록보다 먼저 끝낸다. 이전 앱이 끝난 뒤의 실패(설치 프로그램 검증·설치 실패·Unknown·재시작 실패)와 성공에서는 그대로 두어 스스로 끝나게 한다.
- 핸들과 job
  - runner가 받은 핸들 6개는 받자마자 상속 불가로 바뀐다(`app/src-tauri/src/platform/update_runner/native.rs:316-319`). 유지 연결은 그 뒤에 띄운다.
  - 설치 프로그램 실행(`execute.rs:175-181`)과 같은 방식으로 띄운다: `Command`, null stdio, `CREATE_NO_WINDOW`.
  - 유지 연결을 kill-on-close job에 넣지 않는다. runner는 앱의 job에서 벗어나 있으므로(`native.rs:281` `CREATE_BREAKAWAY_FROM_JOB`), runner가 끝나도 유지 연결은 남았다가 스스로 끝난다.
- 예외
  - 유지 연결을 띄우지 못해도 업데이트는 진행하고 기록만 남긴다.
  - `keep_distro`가 없으면(이전 형식의 인계) 아무것도 하지 않는다.
  - (구현 반영, Ruling 127) 기록은 stderr 한 줄이다(`Record`·`Outcome`은 그대로). `keep_distro`는 빈 값·128바이트 초과·제어 문자·`-` 시작만 거절하고(`valid_keep_distro`), 만드는 쪽도 같은 검사를 써서 연결이 준비되지 않았거나 통과하지 못하면 `None`으로 넘긴다(업데이트는 진행한다).
  - (구현 반영, Ruling 138) `owner.json`은 다음 판이 시작할 때 다시 읽으므로 `keepDistro` 키 이름과 관대한 검사를 주석과 시험으로 고정했다. `wsl.exe`를 runner 폴더보다 System32에서 먼저 찾는 일(M2)은 1.0.x 후속이다(PROGRESS §4).
- 확인 창 문구: "앱을 닫고 업데이트를 설치한 뒤 다시 시작합니다. 설치하는 동안에도 WSL 연결을 유지하므로 WSL의 에이전트 작업과 서비스는 계속 실행됩니다."

**단계**
- [x] 1. 실패 시험을 쓴다.
  - `keep_connection_command_targets_the_bound_distro_without_a_window`
    - 기대: program이 `wsl.exe`, 인자가 `-d <distro> --exec sleep 900`, 창 없음 플래그.
  - `keep_connection_inherits_no_runner_handle`
    - 기대: 유지 연결을 띄운 뒤에도 runner 핸들 6개의 `HANDLE_FLAG_INHERIT`가 0이다.
  - `manifest_without_keep_distro_still_validates`
  - `manifest_round_trips_keep_distro`
  - `update_confirmation_says_wsl_stays_connected`
- [x] 2. 실패를 본다.
- [x] 3. 구현한다.
- [x] 4. 확인한다: 위 시험과 `app/src-tauri` 업데이트 시험이 PASS. Windows 전용 코드는 `Rust (Windows)` CI에서 돈다.
  - (구현 반영, Ruling 128) 시험은 위 다섯과 `manifest_rejects_an_unsafe_keep_distro`다. `keep_connection_inherits_no_runner_handle`은 Windows 전용이라 `Rust (Windows)` CI에서만 돈다. 이 PC에서는 gnu target 대신 설치된 MSVC target clippy로 컴파일만 확인했다(`ring` 빌드에 `AR_x86_64_pc_windows_msvc` 대체가 필요했다).
- [x] 5. 검토 뒤 커밋한다.
- **Windows 확인:** S8-5 앞의 Windows 확인에 더한다. accept 채널에서 tmux 작업이 도는 동안 fixture 업데이트를 실행한다. fixture 업데이트도 같은 runner 경로를 지난다(`execute.rs:155-168`).
  - 확인: 배포판이 멈추지 않는지(tmux 세션 유지), 그리고 연결 공백 시간.
  - interop이 돌아올 때까지 NOT_RUN이다.

#### S8-4b-11 문서

공개 문서와 원장을 이 PR의 코드에 맞춘다. 각 문장은 아래 그대로 쓰되 ui-terms를 따른다.

**README.md**
- :44를 고친다: "재개할 때 도구의 세션 id를 모르면 pi는 지시문 없이 새 pi 세션을 연다. Claude Code·opencode는 처음 지시문으로 새로 시작한다. Codex는 rollout 기록으로 그 작업의 세션을 확인하지 못하면 재개하지 않고 오류를 보인다."
- "에이전트 도구" 절, :34 근처: "Codex 작업은 devbox의 `notify`를 `-c`로 붙여 시작하므로, 그 작업에서는 사용자 config.toml의 `notify`가 쓰이지 않는다. 다른 Codex 실행에는 영향이 없다."
- "도구 경로 찾기" 절: "WSL 쪽 기본 터미널·도구 경로 찾기·환경 기록은 bash 로그인 셸 기준이다. zsh·fish 설정 파일에만 둔 PATH는 읽지 않으므로, 도구 경로는 bash도 읽는 `~/.profile`에 둔다. zsh 터미널은 `[[terminal.profiles]]`로 더할 수 있다(설정 문서 `docs/config.md`로 링크)."
- "알려진 제한"에 더한다.
  - "언어 서버: 코드 파일을 열면 그 프로젝트 폴더에서 언어 서버가 자동으로 시작된다. 프로젝트 실행 승인과 상관없다. 언어 서버와 그 서버가 부르는 도구는 저장소의 빌드 스크립트·proc macro·설정처럼 저장소 안의 코드를 실행할 수 있으므로, 믿지 않는 저장소는 프로젝트로 등록하지 않는다(보안 정책 `SECURITY.md`로 링크)."
  - "Windows 사본은 WSL에서 지우거나 이름을 바꾼 파일을 지우지 않는다. 그 파일이 Windows 빌드·시험에 남을 수 있다."
  - "vault 안의 폴더를 앱 밖에서(파일 탐색기·`mv`·`git pull` 등) 옮기거나 이름을 바꾸면, 그 폴더 안 노트가 새 경로로 색인되지 않아 노트 목록·검색·태그·백링크에서 빠질 수 있다. 파일은 그대로이고, 데몬이 다시 시작할 때 전체를 다시 색인한다."

**SECURITY.md**
- "비밀" 절: "실행 로그와 MCP 로그 응답에서는 저장된 비밀 값의 원문·JSON 이스케이프·디버그 표기를 가린다. 그 밖의 방식으로 바꾼 값(base64·URL 인코딩 등)은 가리지 못한다. 이 판 전에 저장된 실행 로그는 다시 가리지 않는다."
- 새 절 "언어 서버": "코드 파일을 열면 devbox가 그 프로젝트 폴더에서 언어 서버(rust-analyzer·typescript-language-server 등)를 자동으로 띄운다. 이것은 `.devbox/devbox.toml` 실행 승인과 상관없다. 언어 서버와 그 서버가 부르는 도구(cargo 등)는 저장소의 설정과 코드(빌드 스크립트·proc macro 등)를 실행할 수 있다. 그래서 언어 서버가 설치돼 있으면 파일을 여는 것만으로 저장소 안의 코드가 실행될 수 있다. 믿지 않는 저장소는 프로젝트로 등록하지 않는다. 1.0.0은 자동 시작을 유지한다(사용자 결정 2026-11-06)."

**docs/guide.md :84:** "| 노트 | 노트 위치를 바꾸는 방법(`[notes] vault`)과 템플릿 목록 |"

**PROGRESS.md**(200줄 이하 유지, 지금 199줄)
- S8-4 결과: PR #653 → main `08dffcd3`, #652 닫음.
- S8-5 보류: 사용자 결정 2026-11-05 "보류: Windows 확인 뒤".
- 최종 교차 검토 결과와 이 PR, 근거 폴더 위치.
- §4 :90의 Codex 문장을 고친다.
- §4에 1.0.x 후속을 옮긴다: 통합 보고 §4 B 7건, §6 D, 그리고 계획에서 뺀 CB-02(한글 제목). ID와 짧은 이름만 묶어 몇 줄로 적고, 줄 수가 모자라면 끝난 S8-4 기록을 줄인다.
- 근거 폴더 이동은 Ruling 91·CONVENTIONS §8 최종 SHA 원칙의 인계다.

**이 계획(10)**
- S8-4b 체크박스를 고친다.
- S8-3의 accept 스모크 항목에 "S8-4b squash SHA로"를 붙인다.

(구현 반영) 코드와 다른 문장은 뜻을 지키고 고쳤다(근거는 SDD 폴더 `task-S8-4b-11-report.md`): bash 로그인 파일은 `~/.bash_profile`·`~/.bash_login`·`~/.profile` 가운데 먼저 있는 것, 언어 서버는 설치된 것만 시작, 비밀 가림은 그 실행에 넘긴 값이고 로그 파일과 그것을 읽는 앱 화면·`logs.search`·MCP `devbox_logs`에 적용, Codex 설정은 "사용자의 Codex `config.toml`". README 설치·앱 업데이트 절과 guide에 S8-4b-6·S8-4b-10 내용을 더했다.

**단계**
- [x] 1. 위 문서를 고친다.
- [x] 2. 링크 검사와 Biome을 돌린다. 문서가 읽는 시험(`docs/config.md`·`THIRD_PARTY_NOTICES.md`)이 영향을 받으면 그 시험도 돌린다.
- [ ] 3. 검토 뒤 커밋한다.

#### S8-4b-12 브랜치 전체 검토·전체 검사

- [ ] 1. 다른 에이전트가 `08dffcd3..HEAD` 전체를 검토한다. 과제별 검토로 대신하지 않는다. 결과는 PROGRESS 작업 사본에 남긴다.
- [ ] 2. 고칠 것이 있으면 그 과제와 같은 방식(실패 시험 → 구현 → 검토 → 커밋)으로 고친다.
- [ ] 3. `pnpm check`를 한 번 돌린다(clippy·build·E2E 포함, `--wait`).
- [ ] 4. Biome CI를 돌린다.
- [ ] 5. 태그 노트 초안(Git common-dir `devbox-verification/s8/sdd-10-s7-s8-release/s8-3-tag-notes-draft.md`)에 이 PR의 수정과 새 알려진 제한(언어 서버)을 더한다.

#### S8-4b-13 push·PR·리뷰·머지

- S8-4와 같다:
  - 최종 소스 SHA를 push한다.
  - 그 SHA의 브랜치에서 `windows.yml`을 수동으로 실행하고, 산출물을 대조한다(receipt·sha256·source_sha·N1 `-Verify`).
  - PR을 한 번 연다.
  - CONVENTIONS §8의 자동 리뷰 판정을 따른다. 켜짐이면 최대 3회, 꺼짐이면 독립 검토 1회다.
  - 필수 CI 3개가 통과한 뒤 squash 머지한다.
- Windows 수용 스모크는 interop이 돌아올 때까지 NOT_RUN이다(Ruling 106).
- 머지 뒤 S8-5는 새 main SHA로 진행한다.
  1. interop을 한 번 확인한다. 반복 감시는 하지 않는다.
  2. accept 설치 스모크, I1 W1, S8-4b-10 업데이트 확인을 한다.
  3. 결과로 노트·PROGRESS·README 문장을 참으로 고칠 계획을 세운다.
  4. v1.0.0 공개 승인을 다시 묻는다.

### S8-5 1.0.0 공개 — 별도 승인

- [ ] 1. 사용자에게 제시한다: 공개 준비 결과, SC1–10, 13개 확인 결과, I1, 릴리스 노트. 그리고 **v1.0.0 공개 승인**을 묻는다. 사용자는 목록을 모두 직접 실행하지 않고도 승인할 수 있다(07c).
- [ ] 2. 승인 뒤에만 아래를 한다.
  1. main SHA에 주석 태그 `v1.0.0`을 만들고 push한다.
  2. release run이 40분 안에 SUCCESS인지 본다.
  3. asset SHA-256을 확인한다.
  4. `gh api repos/jihoon22-lee/devbox/releases/latest`가 `v1.0.0`인지 확인한다.
- [ ] 3. 사용자가 prod rc.2 위에 1.0.0을 직접 설치한다.
  - 설정 → 정보 → 앱 업데이트 [새 버전 확인]이 "최신 버전"을 보이는지 확인한다. stable 경로의 첫 실제 실행이다(PROGRESS §4 ci-docs M2).
  - 공개 버전 사이의 실제 업데이트는 다음 patch 때까지 NOT_RUN이다.

### S8-6 공개 기록

- [ ] 문서 전용 PR(`docs/v1.0.0-record`, #650 선례)에 담을 것:
  - PROGRESS 공개 결과
  - 이 문서(10)의 보관과 `docs/history.md` 링크
  - 계획 10을 가리키는 상대 링크(README·CONVENTIONS·AGENTS·docs/history.md·PROGRESS, 2026-11-04 기준 9곳)를 고정 링크로 바꾸고 링크 검사 0건을 확인한다
- [ ] 정리: S8 worktree·브랜치를 정리한다. archive·cache·사용자 자료는 보존한다.
