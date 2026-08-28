# wsl-desktop 터미널 — 정확성·세션·사용성 설계

- 상태: v0.4.1 범위(§2) 구현 및 안정판 배포 완료; v0.5.0 §3·§4 **P1 범위 확정, 구현 전**
- 작성일: 2026-08-17 (범위 개정: 2026-08-22)
- 범위: `apps/wsl-desktop` 단일 앱 + `crates/wsl`
- 관련: [UX 개선 설계](./2026-08-15-ux-improvements-design.md) §2 를 이 문서가 대체한다
- 선행: [앱 간 연동 설계](./2026-08-17-app-interop-design.md) §1.2 (`Path` 수신은 v0.4.1,
  `Profile` 수신과 레이아웃 선택은 v0.5.0 §4.4)

> **2026-08-22 native-first 확인.** 탭·팬·profile·layout·action palette는 WSL Desktop이
> 자체 제공하며 tmux/zellij 설치를 전제로 하지 않는다. 멀티플렉서는 앱 종료 뒤 process
> 생존이 필요한 사용자를 위해 이미 설치된 session에 attach하는 optional adapter다. 앱이
> 자동 설치하거나 없을 때 core terminal 기능을 비활성화하지 않는다. 전체 우선순위와
> offline 계약은 [v0.5.0 계획](./2026-08-22-v0.5.0-native-first-plan.md)을 따른다.

## 0. 배경

wsl-desktop은 13개 앱 중 사용 빈도가 가장 높지만, 실사용에서 "중간 중간 화면이 깨진다"는
보고가 반복됐다. UX 개선 설계 문서는 이를 복사/붙여넣기 문제로 보고 §2에 표 6줄로 다뤘으나,
코드를 읽어 확인한 결과 **원인은 UX가 아니라 PTY 전송 계층의 결함**이었다.

이 문서는 그 결함을 먼저 고정하고, 그 위에 세션 유지와 터미널 사용성을 쌓는다.

원칙:
- 결함 수정(§2)과 기능 추가(§3~§5)를 릴리스로 분리한다 — v0.4.1 / v0.5.0
- 순수 함수로 분리 가능한 것은 분리해 테스트한다 (`lib/shortcuts.ts` 선례)
- 세션 지속성은 opt-in이며, 그 대가를 문서에 명시한다

---

## 1. 실측 — 이 명세를 결정한 사실들

이 절의 표와 분석은 v0.4.1 수정 전 v0.4.0/pre-fix 구현을 기록한 역사적 기준선이다. 현재
v0.4.1 동작은 §2에서, Windows acceptance로 남은 확인은 §5에서 구분한다.

코드를 직접 읽어 확인했다. **여기서 나온 사실들이 아래 설계를 결정한다.**

| # | 확인한 것 | 위치 |
|---|---|---|
| 1 | PTY 리더가 4096바이트 읽기마다 `String::from_utf8_lossy` 호출 | `terminal.rs:113-118` |
| 2 | `windowsPty`·`scrollback`·`allowProposedApi` 미설정. 애드온은 `addon-fit` 하나뿐 | `TermPane.tsx:71-76`, `package.json` |
| 3 | `if (rows <= 0 \|\| cols <= 0) return` 은 **죽은 코드** — FitAddon은 최소 1행 2열로 clamp하므로 0이 나오지 않는다 | `TermPane.tsx:93` |
| 4 | `.toolbar`에 `flex-wrap: wrap`, `flex-shrink: 0` 없음. `.terminal-area`에 `min-height: 0` 없음. `.panes`에 바닥값 없음 | `App.css:37-45, 195-203, 291-296` |
| 5 | `lastSizeRef`를 resize IPC **전에** 기록. `.catch` 없음, 재시도 없음 | `TermPane.tsx:95-97` |
| 6 | 세션 id가 밀리초 타임스탬프 (`format!("s{}", millis)`) | `terminal.rs:89-95` |
| 7 | 리더 스레드가 `start_session` 반환 전에 방출 시작. 프론트 핸들러 등록은 `TermPane` 마운트 후 | `terminal.rs:112`, `TermPane.tsx:123`, `App.tsx:132` |
| 8 | `Ok(0) \| Err(_) => break` — 일시 오류를 EOF와 동일 취급 | `terminal.rs:116` |
| 9 | cwd를 `bash -lc "cd '{dir}' && exec bash"` 문자열로 조립. `crates/wsl`의 `--cd` 지원을 쓰지 않음 | `terminal.rs:57-66` vs `crates/wsl/src/argv.rs:20-23` |
| 10 | `grid` 분기의 `gridRows`에 `Math.max(1, …)` 가드 없음 (`cols`/`rows` 분기엔 있음) | `PaneCanvas.tsx:50` |
| 11 | 모든 팬 제목이 distro 이름. `onTitleChange` 미구독 | `PaneCanvas.tsx:72` |
| 12 | 탭/팬 구성이 영속화되지 않음. `storage.ts`는 cwd 핀과 최근 경로 5개만 저장 | `lib/storage.ts` (40줄) |
| 13 | `list_sessions` 커맨드와 `api.ts` 래퍼는 있으나 프론트에서 호출하는 곳이 없음 | `api.ts:68-71` |

### 1.1 함정 — "화면이 깨진다"의 정확한 메커니즘

**`read()`는 4096바이트를 채워서 주지 않는다.** 파이프에 현재 있는 만큼만 반환하므로
**경계는 매 read마다 생긴다.** 4096바이트마다 한 번이 아니다.

`한` = `EA 95 9C` (3바이트, 폭 2)가 `EA 95 | 9C`로 잘리는 경우를 추적하면:

| 청크 | `from_utf8_lossy` 결과 |
|---|---|
| N (`…EA 95`) | 유효한 접두사인데 입력이 끝남 → `error_len() == None` → **U+FFFD 하나** |
| N+1 (`9C…`) | `9C`는 유효한 시작 바이트가 아님 → `error_len() == Some(1)` → **U+FFFD 하나** |

폭 2짜리 글자 하나가 폭 1짜리 `�` 두 개가 된다. **셸과 xterm의 컬럼 모델이 그 줄부터
어긋나고**, 이후 줄바꿈이 전부 밀린다. 반대 방향(`EA | 95 9C`)으로 잘리면 고아 연속
바이트 두 개가 각각 오류를 내 U+FFFD가 **세 개** 나온다.

박스 드로잉 문자(`U+2500`–`U+257F`, 3바이트, 폭 1)는 실사용에서 더 자주 걸린다. 1칸이
2~3칸이 되면서 전체 프레임이 밀린다 — `htop`·`vim`·`lazygit`·`fzf`의 테두리가 어긋나는
정확한 원인이다.

**ANSI 이스케이프 시퀀스는 무사하다.** 순수 ASCII(`ESC [ 3 8 ; 5 ; 1 9 6 m`)라
`from_utf8_lossy`가 건드리지 않고, xterm.js의 파서는 상태 기계라 `write()` 호출 간에
파서 상태를 유지한다. `ESC[3`과 `8;5;196m`이 따로 도착해도 정확히 재조립된다.

> **결론: 깨지는 것은 오직 UTF-8 계층이고, 그것도 Rust에서 깨진다.** xterm.js는 자체
> 증분 UTF-8 디코더를 갖고 있어 `Uint8Array`를 주면 이 경계를 정확히 처리한다. Rust에서
> `String`으로 바꾸는 순간 그 능력을 되돌릴 수 없게 버리는 것이다.

### 1.2 함정 — 죽은 가드가 셸 화면을 파괴한다

`TermPane.tsx:93`의 `rows <= 0 || cols <= 0` 가드는 **한 번도 발동하지 않는다.**
FitAddon의 `proposeDimensions()`가 `Math.max(MINIMUM_COLS /*2*/, …)`,
`Math.max(MINIMUM_ROWS /*1*/, …)`로 clamp하기 때문이다. 즉 **1행 2열 resize가 그대로
ConPTY로 전달된다.**

거기까지 찌그러지는 경로:
- `.toolbar`의 `flex-wrap: wrap` + `flex-shrink: 0` 부재 → 창을 좁히면 툴바가 2~3줄로
  줄바꿈되며 `.main`의 높이를 **window resize 이벤트 없이** 30~60px 훔친다
- `.side-panel`은 `flex-shrink: 0`인데 `.terminal-area`는 `flex: 1; min-width: 0` →
  좁은 창에서 사이드 패널이 양보하지 않아 터미널 폭이 0으로 밀린다
- `.panes`는 `flex: 1`에 바닥값이 없어 축소를 전부 흡수한다

셸이 1행 2열 SIGWINCH를 받고 그 크기로 다시 그리면 **이전 화면 내용은 영구 파괴된다.**
창을 되돌려도 복구되지 않는다.

---

## 2. v0.4.1 — 결함 수정

기능 추가 없음. **이미 출시된 것이 제대로 동작하게 만드는 것까지만.**

### 2.1 PTY 바이트 전송 — carry 버퍼

읽기 경계를 넘기는 carry 버퍼를 둔다. 순수 함수로 분리해 테스트한다.

```rust
/// PTY 바이트를 UTF-8로 디코드한다. 읽기 경계에 걸린 불완전한 멀티바이트 시퀀스는
/// carry 에 남겨 다음 청크 앞에 이어 붙인다. 진짜 잘못된 바이트만 U+FFFD 로 치환한다.
///
/// 불완전 시퀀스는 최대 3바이트(UTF-8 최대 4바이트 - 1)이므로 carry 는 자연히 유계다.
fn decode_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> String {
    carry.extend_from_slice(chunk);
    let mut out = String::with_capacity(carry.len());
    let mut start = 0usize;
    loop {
        match std::str::from_utf8(&carry[start..]) {
            Ok(s) => {
                out.push_str(s);
                start = carry.len();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&carry[start..start + valid]).unwrap());
                match e.error_len() {
                    // 진짜 불량 바이트 — U+FFFD 로 치환하고 건너뛴다
                    Some(n) => {
                        out.push('\u{FFFD}');
                        start += valid + n;
                    }
                    // 끝이 잘린 시퀀스 — 다음 청크로 넘긴다
                    None => {
                        start += valid;
                        break;
                    }
                }
            }
        }
    }
    carry.drain(..start);
    out
}
```

리더 스레드는 `let mut carry = Vec::new();`를 루프 밖에 두고 매 read마다 호출한다.

**검토한 대안과 탈락 사유:**

| 대안 | 사유 |
|---|---|
| `Vec<u8>` 이벤트 | Tauri v2는 serde_json으로 직렬화 — 바이트당 최대 4문자(`"255,"`)라 **4~6배 팽창**. `cargo build` 로그 같은 스트림에 부적합 |
| base64 페이로드 | 1.33배로 양호하고 xterm 자체 디코더를 쓸 수 있으나, Rust 의존성 + 프론트 변경 + 프로토콜 변경이 따른다 |
| `tauri::ipc::Response` 원시 바이트 | push → pull 아키텍처 변경. 이벤트 기반 구조를 전부 뒤집는다 |

**carry 버퍼를 택한다.** 새 의존성 0, 프로토콜 변경 0, 프론트 변경 0, ~25줄, 완전히 정확.
처리량이 실측으로 문제가 되면 그때 base64로 옮긴다.

### 2.2 xterm 옵션

```ts
const term = new Terminal({
  fontSize: 13,
  fontFamily: '"Cascadia Code", Consolas, monospace',
  theme: THEME,
  cursorBlink: true,
  scrollback: 10000,                                  // 기본 1000
  allowProposedApi: true,                             // Unicode11Addon 전제
  windowsPty: { backend: "conpty", buildNumber },     // ConPTY 소프트랩 휴리스틱
});
term.loadAddon(new Unicode11Addon());
term.unicode.activeVersion = "11";
```

- **`windowsPty`가 핵심이다.** ConPTY는 오른쪽 여백에서 하드 랩할 때 랩 플래그를 주지
  않고 다음 행에 이어서 낸다. 이 옵션 없이는 긴 줄이 전부 독립적인 하드 줄로 저장되고,
  **`cols`가 바뀔 때마다(창 크기 변경·사이드 패널 토글·탭 전환·툴바 줄바꿈) 기존 출력
  전체가 리플로우되지 않아 눈에 띄게 망가진다.** 복사 시 가짜 개행도 끼어든다
- `buildNumber`는 Rust에서 한 번 조회해 프론트로 넘긴다
- **Unicode11**이 없으면 한글·이모지·powerline 글리프가 폭 1로 계산돼 §1.1과 같은 종류의
  컬럼 드리프트가 생긴다. 원인만 다르고 증상은 같다
- xterm v6 기준 애드온 버전 호환은 설치 시 확인한다

### 2.3 팬 붕괴 방지 — CSS 바닥값 + JS 클램프

**CSS** (`App.css`):

| 선택자 | 추가 |
|---|---|
| `.toolbar` (37-45) | `flex-shrink: 0` |
| `.tab-bar` (115-123) | `flex-shrink: 0` |
| `.error` (188-193) | `flex-shrink: 0` |
| `.terminal-area` (291-296) | `min-height: 0` |
| `.panes` (195-203) | `min-height` / `min-width` 바닥값 |

**JS** (`TermPane.tsx`) — 죽은 가드를 실제 바닥값으로 교체:

```ts
// FitAddon 은 최소 1행 2열로 clamp 하므로 0 검사는 발동하지 않는다.
// 바닥값 미만이면 전송하지 않고 lastSizeRef 도 갱신하지 않는다 —
// 다시 커졌을 때 재전송되어야 한다.
if (rows < MIN_ROWS || cols < MIN_COLS) return;
```

### 2.4 resize 프로토콜 — ack 후 커밋

```ts
const seq = ++seqRef.current;
resizeSession(sessionId, rows, cols)
  .then(() => {
    // 더 새로운 resize 가 이미 나갔다면 이 응답으로 커밋하지 않는다
    if (seq === seqRef.current) lastSizeRef.current = { rows, cols };
  })
  .catch(() => {
    /* 커밋하지 않음 → 다음 fit 이 재시도한다 */
  });
```

v0.4.0/pre-fix 코드는 `lastSizeRef`를 IPC **전에** 낙관적으로 기록하고 `.catch`도 재시도도 없었다.
resize 한 번이 유실·재정렬되면 이후 모든 `fitAndSendResize`가 `:95`의 조기 반환에 걸려
**xterm과 PTY가 영원히 다른 크기로 남는다.** 사용자가 창을 물리적으로 다른 크기로 바꿔야만
풀린다.

두 트리거 경로가 경합하지 않게 한다:
- `ResizeObserver` → 100ms 디바운스 (`:125-128`)
- 활성 탭 전환 → rAF (`:148-152`) — **여기서 `clearTimeout(resizeTimerRef.current)`를
  먼저 호출**해 대기 중인 디바운스를 취소한다

### 2.5 세션 식별자 — 팬 정체성과 분리

`terminal.rs:89-95`의 밀리초 타임스탬프는 같은 밀리초에 두 세션이 생기면 `HashMap`을
덮어쓴다. 결과: 먼저 들어간 `SessionHandle`이 drop되며 그 ConPTY가 닫히고, 두 리더
스레드가 **같은 `session_id`로 방출해 두 스트림이 한 xterm에 섞인다.**

`AtomicU64` 카운터로 교체한다. 프론트의 `lib/id.ts` `makeId`가 이미 같은 패턴을 쓴다.

동시에 **팬 정체성과 세션 정체성을 분리한다** — §4.1 레이아웃 복원의 전제 조건이다.

```ts
type Pane = {
  key: string;              // 영속. 프론트가 makeId 로 생성. 레이아웃 저장·멀티플렉서 세션명에 사용
  sessionId: string | null; // 런타임. 백엔드 카운터. 재시작마다 새로 부여
  distro: string;
  cwd?: string;
};
```

### 2.6 시작 시 출력 유실 — `attach_session`

리더 스레드가 `terminal.rs:112`에서 `start_session` 반환 **전에** 방출을 시작하는데,
프론트 핸들러 등록은 `TermPane` 마운트 후다(`TermPane.tsx:123`). 그 사이 출력은
`App.tsx:132`의 옵셔널 체이닝(`writes.current.get(id)?.(data)`)으로 **조용히 버려진다.**

유실 자체보다 **어디서 잘리는지**가 문제다. 이스케이프 시퀀스 중간에서 잘리면 처음 도착한
청크가 시퀀스 한가운데서 시작하고, xterm은 그 꼬리를 리터럴 텍스트로 렌더한다
(`0;5;196mhello` 같은 쓰레기).

수정: 리더 스레드 spawn을 새 `attach_session(session_id)` 커맨드로 옮긴다. `TermPane`이
`registerWrite` 직후에 호출한다. 그 사이의 출력은 ConPTY 내부 버퍼가 보관하므로 유실이
없다. attach가 오지 않아도 `close_session`이 정리하도록 유지한다.

### 2.7 나머지

| 위치 | 수정 |
|---|---|
| `terminal.rs:116` | **v0.4.0/pre-fix 기준:** `Interrupted`/`WouldBlock`을 EOF처럼 처리해 일시 오류에 팬이 사라지고 `wsl.exe`가 고아로 남을 수 있었다. **v0.4.1 구현:** 두 일시 오류는 계속 읽고, 루프가 끝나면 현재 핸들과 일치하는 세션만 한 번 정리해 stale reader가 교체 세션을 지우지 못하게 한다 |
| `terminal.rs:57-66` | 문자열 조립 제거 → `build_exec_argv(&distro, Some(dir), "")`의 `--cd` 사용. 경로에 작은따옴표가 있으면 깨지고, `'; cmd; '` 주입 표면이 열려 있다. §4.2 워크스페이스 정의(파일에서 읽는 cwd)가 들어오면 신뢰 경계가 달라지므로 먼저 닫는다 |
| `PaneCanvas.tsx:50` | `Math.max(1, …)` — `repeat(0, 1fr)`은 무효 CSS라 이전 렌더의 `gridTemplateRows`가 남는다. #189이 `cols`/`rows` 분기에 있던 가드를 `grid` 분기에서 누락하며 도입 |
| `App.css:261-268` | `.empty`의 `display: flex`가 `<td>`(`DistroPanel.tsx:120-126`)에 적용돼 `colSpan={5}`가 무효화된다. `.side-panel .empty`에 `display: table-cell` |
| `parsers.rs:23` | `let _state = parts.next()` 폐기 → `DistroInfo`에 state 추가. `DistroPanel.tsx:62`가 모든 distro를 `● Running`으로 하드코딩 중이라 Stopped 배포판도 실행 중으로 보인다 |

---

## 3. v0.5.0 — 클립보드·단축키

### 3.1 자동 복사를 기본으로 켠다

원 문서 §2-4는 "선택 시 자동 복사 (Windows Terminal 동작)"이라 적었는데, WT의
`copyOnSelect`는 **기본 false인 opt-in 설정**이다. 그 서술은 부정확하다.
다만 **동작 자체는 기본 켬으로 간다.**

이 결정이 Ctrl+C 설계를 단순하게 만든다. 원 문서 §2-2는 "`hasSelection()`이면 복사,
아니면 SIGINT" 분기를 제안하며 "readline SIGINT 동작을 망가뜨리지 않도록 주의"라고
경고했다. 그런데 **자동 복사가 켜져 있으면 선택이 생긴 시점에 복사가 이미 끝나 있다.**
분기할 이유가 없다.

| 입력 | 동작 |
|---|---|
| 드래그로 선택 | 즉시 복사 (**기본 켬**, 설정으로 끌 수 있음) |
| `Ctrl+C` | **항상 SIGINT.** 분기 없음 |
| `Ctrl+Shift+C` | 선택이 있으면 명시적 복사 (자동 복사를 끈 사용자용) |
| `Ctrl+Shift+V` | 붙여넣기 |
| 가운데 클릭 | 붙여넣기 |
| 우클릭 | **컨텍스트 메뉴** (§3.3) |

`hasSelection()` 분기를 없애면 원 문서가 경고한 위험이 구조적으로 사라진다. 선택이 남아
있어도 Ctrl+C는 언제나 SIGINT다.

트레이드오프: 실수로 드래그하면 클립보드가 덮어써진다. WT가 기본 off인 이유다. 설정
토글을 제공하고, 껐을 때는 `Ctrl+Shift+C`가 유일한 복사 수단이 된다.

### 3.2 클립보드 채널

원 문서 §2-5의 "`navigator.clipboard` 또는 `@tauri-apps/plugin-clipboard-manager` (+ CSP)"에서
**"CSP"는 삭제한다.** CSP는 클립보드 게이트가 아니다. WebView2가 `readText()`에 사용자
활성화/권한을 요구하는 것이 실제 제약이다.

- **쓰기**(`writeText`)는 이미 저장소 5곳에서 동작한다 (api-playground 2, developer-toolbox 2,
  everything-plus 1). 그대로 쓴다
- **읽기**(붙여넣기)만 `@tauri-apps/plugin-clipboard-manager`를 도입한다. Cargo.toml +
  package.json + `capabilities/default.json` + `lib.rs`의 `.plugin(...)` 네 곳을 건드린다.
  현재 wsl-desktop의 capability는 `["core:default", "opener:default"]`뿐이다
- 붙여넣기는 `writeSession` 직접 호출이 아니라 **`term.paste()`**를 쓴다 — bracketed paste가
  적용돼 셸이 붙여넣은 텍스트를 리터럴로 다룬다

**안전 경계**: bracketed paste를 지원하지 않는 셸/TUI에서는 여러 줄 붙여넣기가 즉시
실행된다. **개행이 포함된 붙여넣기는 확인 프롬프트**를 띄운다.

### 3.3 우클릭 = 메뉴 (충돌 해소)

원 문서 §2-3("우클릭에서 preventDefault 후 클립보드를 PTY에 전송")은 §1(13개 앱 우클릭
메뉴)과 정면 충돌한다. §1.2 표에 wsl-desktop 행이 없어(13개 중 12개만 수록) 이 모순이
표면화되지 않았다.

**결정: 우클릭 = 메뉴, 가운데 클릭 = 붙여넣기.**

터미널 팬의 메뉴 항목 (이 앱 고유 — 다른 앱과 공유하지 않는다):

| 항목 | 조건 |
|---|---|
| 복사 | 선택이 있을 때만 활성 |
| 붙여넣기 | — |
| 검색 | §4.3 |
| 세로 분할 / 가로 분할 | — |
| cwd 복사 | OSC 7 로 cwd 를 알 수 있을 때 |
| 팬 닫기 (danger) | — |

> **2026-08-26 구현 상태 (#260, #262).** 공용 `@devbox/context-menu`에 팬·탭 trigger를 연결하고
> 팬의 세로/가로 분할·확인 후 닫기와 탭의 닫기·다른 탭 닫기·이름 변경·레이아웃
> 전환을 구현했다. 기존 팬/탭 close button과 `Ctrl+Shift+W`도 같은 danger confirmation
> 경로를 쓴다. 팬 메뉴가 닫히면 DOM root에서 멈추지 않고 registry의 xterm `focus()`를
> 호출한다. #262는 팬 handle registry에서 우클릭한 exact 팬의 selection과 유효한 OSC 7
> cwd를 snapshot해 복사/cwd 복사 활성 조건을 정하고, 붙여넣기·검색까지 같은 exact 팬에
> 전달한다. 읽기 권한 또는 clipboard 호출이 실패해도 PTY input/세션은 유지하며 raw 오류를
> 화면에 반향하지 않는다.

---

## 4. v0.5.0 — 세션과 사용성

### 4.1 레이아웃 복원 (기본 동작)

현재 `storage.ts`(40줄)는 cwd 핀과 최근 경로 5개만 저장한다. 탭/팬 구성은 앱을 닫으면
사라진다. 같은 파일의 선례를 따라 localStorage에 저장한다.

```ts
type PersistedLayout = {
  version: 1;
  tabs: { id: string; title: string; layout: "grid" | "cols" | "rows"; paneKeys: string[] }[];
  panes: { key: string; distro: string; cwd: string | null; persistent: boolean }[];
  activeTabId: string;
};
```

- 복원 대상: 탭 구성, 팬 배치, 레이아웃 모드, cwd, 활성 탭
- 복원 안 함: 스크롤백, 실행 중이던 프로세스 (그건 §4.2)
- §2.5의 `paneKey`/`sessionId` 분리가 전제다

**제약**: `PaneCanvas.tsx:19-33`의 주석이 설명하는 불변식 — `panes` 배열 순서는 절대
바뀌면 안 된다(바뀌면 React가 fiber를 옮겨 xterm이 재마운트되고 스크롤백을 잃는다).
따라서 복원은 **초기 마운트에서 배열을 한 번 구성**하고, 이후에는 기존대로 append/filter만
쓴다.

### 4.2 멀티플렉서 opt-in (프로세스 생존)

앱을 닫아도 빌드·dev server·ssh가 살아 있게 하려면 멀티플렉서가 필요하다. `wsl.exe`가
종료되면 셸에 SIGHUP이 가므로 **자체 데몬을 만들지 않는 한 다른 방법이 없다.**
opt-in으로 둔다. 레이아웃 복원과 workspace 정의는 §4.1·§4.4의 native 기능이므로
멀티플렉서가 없어도 완전하게 동작하고, process 생존만 제공되지 않는다.

백엔드 트레이트는 WSL Desktop에 필요한 detect/list/attach/kill 동작을 기준으로 독립
설계한다. 외부 프로젝트의 구현·parser·정규식을 복사하지 않는다.

```rust
pub struct MuxSession { pub name: String, pub state: MuxState }  // Running | Exited

pub trait Multiplexer {
    fn available(&self, distro: &str) -> Option<String>;              // 경로/버전
    fn list(&self, distro: &str) -> Result<Vec<MuxSession>, String>;
    fn ensure_argv(&self, name: &str, cwd: Option<&str>) -> Vec<String>;
    fn kill(&self, distro: &str, name: &str, purge: bool) -> Result<(), String>;
}
```

- **세션 이름**: `wsld-<distro>-<paneKey>`. `paneKey`가 영속(§2.5)이므로 재시작을 넘어
  안정적이다
- **zellij UI 숨김**: 공식 zellij KDL 설정을 사용해 `tab-bar`/`status-bar` plugin pane을
  만들지 않고 `pane_frames false`를 적용한다. 결과적으로 zellij는 화면에 보이지 않고,
  탭·분할·단축키는 WSL Desktop이 계속 소유한다
- **tmux**: 같은 트레이트 뒤에 `new-session -A -s <name> -c <cwd>` + `set -g status off`
- **감지/폴백**: 멀티플렉서가 없으면 토글을 비활성화하고 사이드 패널에 사유를 표시한다.
  기존 동작(비지속 세션)으로 조용히 내려간다
- list parser는 공식 CLI의 machine-readable 또는 stable 출력만 대상으로 fixture를 직접
  작성한다. 단순 session 이름뿐 아니라 running/exited 상태를 함께 검사해 잔재를 새 session으로
  오판하지 않는다

**정직한 트레이드오프 — 이것 때문에 opt-in이다:**

| 잃는 것 | 설명 |
|---|---|
| 스크롤백 소유권 | 멀티플렉서로 넘어간다. §4.3의 xterm 검색은 mux 모드에서 **화면에 그려진 것만** 본다 |
| 마우스 선택 | mux의 마우스 모드를 꺼야 xterm이 선택을 유지한다 |
| (잃지 않음) 앱 단축키 | `Ctrl+Shift+*`는 `attachCustomKeyEventHandler`가 PTY 전송 **전에** 가로채므로 그대로 동작한다. mux 자체 prefix(`Ctrl+B` 등)는 통과하며 이는 의도된 동작 |

### 4.3 사용성 기본기

| 항목 | 내용 |
|---|---|
| 팬/탭 제목 | `term.onTitleChange`(OSC 0/2) 구독 → 실행 중인 명령·cwd 표시. 현재 `PaneCanvas.tsx:72`가 모든 팬을 distro 이름으로 표시해 **팬 4개면 전부 "Ubuntu"** 다 |
| 스크롤백 검색 | `@xterm/addon-search` + `Ctrl+Shift+F` |
| 링크 | `@xterm/addon-web-links` — 출력의 URL 클릭 |
| 렌더러 | `@xterm/addon-webgl` (실패 시 canvas → DOM 폴백) |
| 폰트 크기 | `Ctrl` `+`/`-`/`0`, 영속화. 현재 `THEME`·`fontSize` 하드코딩 |
| 팬 크기 조절 | grid/cols/rows 프리셋에 더해 드래그 리사이즈 |

> **2026-08-26 #262 구현 상태.** OSC 0/2 팬 제목과 활성 팬 기반 자동 탭 제목(수동 이름
> 우선), OSC 7 cwd, addon-search, OSC 8 core link handler와 addon-web-links, 영속 글꼴
> 크기를 구현했다. 링크는 HTTP(S)만 허용하고 embedded credential을 거부하며 host 확인 뒤
> 연다. 기존 ConPTY wrap·10,000줄 scrollback·resize retry/hidden-pane 바닥값 회귀 테스트를
> 함께 유지한다. WebGL renderer와 드래그 팬 크기 조절은 P1-07 issue의 acceptance에 포함하지
> 않으며 native workspace/profile·multiplexer와 함께 이 PR로 끌어오지 않는다.

### 4.4 워크스페이스 정의와 명령 팔레트

[앱 간 연동 설계](./2026-08-17-app-interop-design.md)의 `wsl-desktop --profile <id>`가 의미를
가지려면 **"이 프로젝트의 터미널 레이아웃"** 개념이 필요하다 — 탭 2개, 팬 3개, 각각의 cwd와
시작 명령. 이 정의는 WSL Desktop 소유 JSON schema로 만들고 외부 workspace 파일에 의존하지 않는다.

**터미널 사용성 강화와 앱 간 유기성이 같은 부품을 요구하는 지점이다.**

- 워크스페이스 정의(탭/팬/cwd/시작 명령)를 `app_local_data_dir`의 JSON에 저장한다.
  레이아웃(§4.1)이 "마지막 상태 자동 저장"이라면, 워크스페이스는 "이름 붙여 저장한 정의"다
- 세션 목록 패널: `defined`(정의만) / `running` / `exited`(부활 가능)의 3상태.
  정의에 없는 살아있는 세션도 보여준다(손으로 만든 것을 놓치지 않기 위해)
- **명령 팔레트**: WSL Desktop 소유 snippet model. `{{param}}` 치환 시 **기본적으로
  인용하고**, 값 자체가 shell fragment여야 하는 경우만 `raw = true`로 명시적 opt-in 한다.
  최종 command를 실행 전에 보여준다

**책임 경계 — 문서에 못박는다** (UX 개선 설계 §0의 "각 앱의 책임을 다른 앱에서 복제하지
않는다"):

| 앱 | 소유 |
|---|---|
| run-manager | 예약·백그라운드·서비스 (스케줄된 실행) |
| wsl-desktop 스니펫 | 지금 이 팬에 대화형으로 보내기 (결과가 화면에 남는다) |
| workbench | *프로젝트* 프로필 (경로·포트·서비스) |
| wsl-desktop 워크스페이스 | *터미널 레이아웃*. cwd 기본값은 `crates/integration` 스냅샷으로 workbench에서 읽는다 |

> **2026-08-26 #263 구현 상태.** runtime `sessionId`와 stable `paneKey`를 분리한 상태에서
> 탭·팬·distro·안전한 절대 cwd·layout·한 줄 시작 명령을 마지막 레이아웃(localStorage)과
> 이름 있는 profile(`app_local_data_dir/terminal-profiles.json`)로 각각 저장·복원한다. profile
> store는 version 1 전체 참조를 검증하고 손상·중복·orphan·unsafe path·명백한 raw credential을
> fail-closed 처리하며 atomic replace만 사용한다. cold/hot `OpenTarget::Profile`은 같은 실행
> 경로로 수렴하고, 시작 명령은 최종 문자열을 보여 준 뒤 새 세션에 한 번만 전달한다.
>
> native layout과 action palette(split/close/search/profile switch/cwd copy)는 외부 도구 없이
> 완전하게 동작한다. tmux/zellij는 설치 여부를 exact argv로 읽기 전용 감지한 뒤 stable
> `wsld-*` 세션에 opt-in attach/create할 뿐 설치·download하지 않으며, 없거나 감지가 실패하면
> backend에서도 native로 내린다. tmux option은 해당 session에만 적용하고 zellij는 공식
> `disable-status` layout과 frame/mouse off option을 사용한다. broadcast는 기본 off, 활성 탭의
> 팬을 사용자가 2개 이상 직접 선택해야 활성화되며 대상 수를 계속 표시한다. multiline paste와
> 위험 명령 Enter는 raw command를 오류/확인문에 반향하지 않고 대상 수와 실행 위험을 다시
> 확인한다. W1 packaged build 실기 checkpoint는 남아 있다.

### 4.5 Resource summary와 broadcast safety의 공통 snapshot (#344/#345)

두 기능은 화면상 별개처럼 보이지만 같은 distro/session generation을 표시해야 한다. 따라서
backend는 `dashboard_snapshot` 하나에서 다음 순서로 수집한 값을 `revision`,
`capturedAtMs`, `staleAfterMs`와 함께 반환한다.

```text
wsl.exe -l -v
  └─ running distro마다
      ├─ docker ps -a --no-trunc --format (ID, name, image, status, ports)
      ├─ cat /proc/stat
      ├─ cat /proc/meminfo
      └─ df -P -B1 -- /
```

모든 child는 고정 argv·stdin null·5초 deadline·bounded stdout/stderr를 사용하고, 전체 collection도
30초 deadline으로 제한한다. shell,
`bash -lc`, 사용자 명령, 환경 확장, engine/resource 설치는 없다. stopped distro는 resource나
Docker query를 위해 깨우지 않는다. resource parser는 연속 정상 `/proc/stat` aggregate counter의
delta CPU 사용률과 memory/disk used/total만 numeric field로 보존한다. 첫 CPU 표본이나 counter
reset에는 거짓 비율을 만들지 않고 `null`을 반환하며 checked arithmetic와 JavaScript safe-integer
상한을 적용한다. Docker
detail은 dashboard 메모리에서만 사용하고 runtime integration snapshot에는 정규화된 state/name/
hex ID/공개 port mapping만 남긴다. malformed, partial, timeout, overflow와 session count
불일치는 빈 성공 결과가 아니라 last-good atomic envelope로 격리한다.

`SnapshotCoordinator`의 collection lock은 background runtime writer와 manual/lifecycle dashboard
refresh의 single-flight 경계다. frontend는 snapshot TTL에 맞춘 자동 poll도 같은 promise 경계로
합치며, 한 `DashboardSnapshot`에서 distro card, active
terminal count, resource label, selected Docker list를 함께 파생한다. broadcast target은 unique
pane ID를 최소 2개·최대 32개까지 명시적으로 선택하며, `<`, `>`, `<<`, `>>` redirection은
공백 유무와 관계없이 danger confirmation을 요구한다. `refreshing` 중에는 Refresh 버튼이
재진입하지 않고, stale/error/refreshing 또는 Docker/workspace/context action
busy 중에는 마지막 정상 panel을 표시하되 broadcast를 자동 OFF하고 target checkbox를 잠근다.
단일 terminal의 PTY read/write는 이 상태와
무관하게 계속 허용한다. refresh promise의 sequence guard는 rapid navigation에서 이전 distro의
resource/container 상태가 새 선택에 재삽입되는 것을 막는다.

#344의 독립 acceptance는 numeric resource fixture, Docker available/missing/empty/error,
active-terminal count, parser/byte/CPU/memory/disk bounds, timeout·poll failure·last-good이다.
#345의 독립 acceptance는 기본 OFF, active pane selector, target count badge, multiline paste와
`sudo`/`rm`/shell redirection danger confirmation, cancel 후 재확인, keyboard/focus/a11y label이다.
두 issue는 이 snapshot/rollback fixture를 공유하므로 grouped PR로 검토하지만, destructive
Docker action 추가·arbitrary shell·외부 설치·#307 Knowledge handoff는 포함하지 않는다.

---

## 5. 테스트 계획

| 대상 | 방법 |
|---|---|
| `decode_chunk` | **골든 테스트.** 한글·박스드로잉·이모지를 1~3바이트 경계 **모든 위치**에서 분할해, 재조립 결과가 원본과 바이트 동일한지. 진짜 불량 바이트는 U+FFFD가 정확히 하나만 나오는지. carry가 3바이트를 넘지 않는지 |
| resize 커밋 | **v0.4.0/pre-fix 기준:** `resizeSession` reject 뒤에 낙관적으로 기록한 크기가 남아 재시도되지 않았다. **v0.4.1 구현:** ack가 올 때만 `lastSizeRef`를 커밋하고, reject 시에는 커밋하지 않아 다음 `fitAndSendResize`가 같은 크기를 재전송하는지 단언 |
| resize 경합 | 디바운스 타이머가 걸린 상태에서 탭 전환 → `clearTimeout`이 호출되는지 |
| 바닥값 | 바닥값 미만 크기에서 `resizeSession`이 호출되지 않고 `lastSizeRef`도 갱신되지 않는지 |
| 세션 id | 동시 `start_session` N회 → id 중복 0 |
| `PaneCanvas` | 활성 탭에 팬이 없을 때 `gridTemplateRows`가 `repeat(0, …)`가 아닌지 (기존 `PaneCanvas.test.tsx` 확장) |
| 레이아웃 복원 | 직렬화 → 역직렬화 왕복. 손상된 JSON·버전 불일치는 빈 상태로 폴백 |
| 멀티플렉서 백엔드 | `Command` 호출을 가로채 argv 검증. **plugin pane이 없는 레이아웃을 쓰는지 명시적으로 단언**(§4.2 회귀 방지). `list` 파서는 실제 출력 3종(running/EXITED/없음) fixture |
| 단축키 | `matchShortcut`에 `Ctrl+Shift+C/V/F` 추가 케이스 (`shortcuts.test.ts` 확장) |
| resource parser/collection | `/proc/stat`, `/proc/meminfo`, `df -P -B1 -- /` numeric fixture와 first/delta/reset CPU, memory/disk bounds, malformed/timeout/last-good 경로. 고정 argv에 shell·임의 command·path가 없는지 단언 |
| dashboard single-flight | background writer와 manual refresh가 하나의 collection lock/revision을 공유하는 fixture. same snapshot의 distro/state/resource/container/terminal count를 반환하고 refresh 중 두 번째 버튼 호출을 막는지 단언 |
| broadcast safety | target count 0/1/2+, 기본 OFF, multiline paste, `sudo`·`rm`·redirection 확인 및 취소 후 재확인. loading/refreshing/stale/error에서는 target/ON을 차단하되 단일 PTY I/O가 살아 있는지 단언 |
| snapshot navigation | 이전 promise가 늦게 도착하는 rapid refresh/selected-distro 전환 fixture에서 stale response와 이전 container/resource가 폐기되는지, Unicode label과 keyboard focus가 유지되는지 단언 |

**Windows 실기 검증 (WSL에서 불가능 — ConPTY·`windowsPty`·WebView2 클립보드는 전부
Windows 전용 경로다).** `CONVENTIONS.md §1`의 "편집은 WSL, 빌드·실행은 Windows" 규약대로
`pnpm tauri dev`로 확인한다:

- `find / 2>/dev/null | head -100000` 대량 출력 후 `�` **0개**
- `htop`·`lazygit`·`vim`을 띄운 채 창 크기 반복 변경 → 프레임 어긋남 없음
- 창을 최소 폭까지 줄였다 되돌리기 → **v0.4.0/pre-fix 기준:** 1행×2열에 가까운 resize가 셸 화면을 파괴할 수 있었다. **v0.4.1 구현:** 바닥값(4행×20열) 미만은 resize를 전송·커밋하지 않고, 활성화 시 pending ResizeObserver 디바운스를 취소한 뒤 rAF로 다시 맞춘다. Windows 화면 보존 여부는 acceptance에서 확인한다.
- 긴 줄(200자+) 출력 후 resize → 줄바꿈 보존 (`windowsPty` 검증)
- 팬 4개 빠른 연속 생성 → 세션 id 충돌 없음, 출력 섞임 없음
- 한글 프롬프트·이모지 powerline 테마에서 커서 위치 정확 (Unicode11 검증)

이 체크리스트를 v0.4.1 PR 본문에 그대로 넣어 통과 여부를 남긴다.

---

## 6. 구현 순서

의존성이 적고 테스트가 쉬운 것부터. 각 단계는 그 자체로 검증 가능하다.

| # | 작업 | 릴리스 | 완료 기준 |
|---|---|---|---|
| 1 | `decode_chunk` + 골든 테스트 | v0.4.1 | 경계 분할 테스트 전부 통과 |
| 2 | `windowsPty` / `scrollback` / Unicode11 | v0.4.1 | 실기: 긴 줄 resize 보존 |
| 3 | CSS 바닥값 + JS 클램프 | v0.4.1 | 실기: 최소 폭에서 셸 화면 보존 |
| 4 | resize ack + 두 경로 조율 | v0.4.1 | reject 후 재시도 단언 |
| 5 | 세션 id 카운터 + `paneKey` 분리 | v0.4.1 | 동시 생성 중복 0 |
| 6 | `attach_session` | v0.4.1 | 초기 출력 유실 0 |
| 7 | 잔여 결함 (§2.7) | v0.4.1 | clippy + 기존 테스트 통과 |
| 8 | 클립보드·단축키 (§3) | v0.5.0 | 자동 복사 기본 켬, Ctrl+C = SIGINT |
| 9 | 사용성 기본기 (§4.3) | v0.5.0 | 팬 제목이 실행 명령을 반영 |
| 10 | 레이아웃 복원 (§4.1) | v0.5.0 | 재시작 후 탭/팬/cwd 복원 |
| 11 | 멀티플렉서 opt-in (§4.2) | v0.5.0 | 앱 재시작 후 프로세스 생존 |
| 12 | 워크스페이스 + 명령 팔레트 (§4.4) | v0.5.0 | `--profile`로 레이아웃 열림 |
| 13 | Resource summary + broadcast safety (§4.5, #344/#345) | v0.5.0 | shared snapshot/stale guard, numeric resource panel, target/multiline/danger confirmation |

각 항목 1 PR.
