# wsl-desktop 탭·드래그·단축키·경로 고정 — 설계

- 날짜: 2026-08-12
- 브랜치: `feat/wsl-desktop/tabs-and-shortcuts`
- 범위: `apps/wsl-desktop` 단일 앱. PTY resize 선결 수정 + 탭 모델 + 드래그 2종 + 단축키 5종 + open path 핀/최근 경로
- 산출: 이 문서(첫 커밋) + 논리 단위별 구현 커밋 여러 개

## 배경

`apps/wsl-desktop`은 v0.2.0에서 다중 세션 + grid/cols/rows 분할을 넣었지만, 탭 개념이
없어 세션이 늘어날수록 한 화면에 모든 팬이 눌려 들어간다. Windows Terminal 사용자가
기대하는 "탭 안에 분할" 계층을 추가하고, 탭 전환·이동을 마우스(드래그)와 키보드(단축키)
양쪽으로 지원한다. 또한 터미널을 열 때마다 작업 경로를 다시 입력해야 하는 불편을
핀 토글 + 최근 경로로 줄인다.

이 작업을 시작하기 전에 `terminal.rs` 전체(273줄)와 `App.tsx`·`TermPane.tsx`·`api.ts`·
`App.css` 전체를 읽었다. 아래 모든 줄 번호는 이 브랜치를 `origin/main`에서 분기한
시점의 원본 기준이다.

## 선결 과제: PTY resize가 없다

### 현재 상태 (버그)

`terminal.rs:56`의 `start_session`은 `openpty`를 단 한 번 고정 크기로 호출한다.

```rust
// terminal.rs:64-70 (원본)
let pair = pty_system
    .openpty(PtySize {
        rows: 30,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    })
    .map_err(|e| e.to_string())?;
```

이후 세션이 살아있는 동안 이 크기를 바꿀 방법이 파일 전체(273줄)에 없다.
`resize_session`류 명령이 아예 존재하지 않고, `lib.rs`의 `invoke_handler!`에도 등록된
게 없다. 프론트의 `FitAddon.fit()`(`TermPane.tsx:41`, `TermPane.tsx:55`)은 xterm.js가
그리는 셀 수만 다시 계산할 뿐, PTY(커널이 아는 winsize)에는 아무것도 전달하지 않는다.
그 결과 패인 크기가 30×100이 아니면(거의 항상 그렇다) `vim`·`htop`·`less` 등 TUI가
실제 터미널 크기를 잘못 알고 그리기가 어긋난다.

### 왜 탭보다 먼저인가

탭 전환(비활성→활성 시 패인이 처음 보이는 크기로 바뀜), 팬 드래그 이동(다른 탭의
grid로 재배치), 레이아웃 변경(grid/cols/rows)은 전부 패인의 실제 픽셀 크기를 계속
바꾼다. resize 배선 없이 탭을 얹으면 "탭을 바꿀 때마다 vim 화면이 깨진다"는 형태로
이 버그가 상시로, 그것도 더 자주 드러난다. 따라서 탭 모델보다 먼저 고정한다.

### 수정

**백엔드** — `SessionHandle.master`(`terminal.rs:14-23`)에는 이미
`#[allow(dead_code)]`가 붙어 있다. 주석이 설명하듯 이 필드는 v0.2.2에서 ConPTY(HPCON)
수명을 유지하기 위해 "보관만" 하고 읽지는 않는 용도로 추가됐다(`fix(wsl-desktop): keep
conpty master alive to avoid 0xc0000142`, #39). `portable_pty::MasterPty` 트레이트에는
`fn resize(&self, size: PtySize) -> Result<(), Error>`가 있으므로, 이 필드를 실제로
읽어 새 명령을 추가한다.

```rust
#[tauri::command]
pub fn resize_session(
    state: tauri::State<'_, Arc<SessionState>>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    if let Some(h) = sessions.get(&session_id) {
        let h = h.lock().unwrap();
        if let Some(master) = &h.master {
            master
                .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
```

`#[allow(dead_code)]`는 제거한다(필드를 실제로 읽으므로 더 이상 필요 없고, 남겨두면
"안 쓰는 필드"라는 거짓 정보가 된다). `lib.rs`의 `invoke_handler!`에 `resize_session`을
추가한다.

**프론트** — `TermPane`의 `ResizeObserver` 콜백(`TermPane.tsx:53-60`)은 지금
`fit.fit()`만 부르고 끝난다. 여기에 100ms 디바운스로 `resize_session(sessionId, term.rows,
term.cols)` 호출을 추가한다. 디바운스 이유: `ResizeObserver`는 드래그로 창 크기를
바꾸는 동안 프레임마다 여러 번 발화하는데, 매번 IPC로 `resize_session`을 보내면
불필요한 왕복이 쌓인다. 같은 rows/cols면 재전송하지 않는다(마지막으로 보낸 크기를
ref에 캐시).

탭이 비활성화되면 그 컨테이너는 `display: none`이 되고, 이 상태에서 `FitAddon.fit()`은
0×0 근처 값을 계산한다(`getComputedStyle` 기반이라 박스가 없으면 크기가 없다).
`rows<=0 || cols<=0`이면 계산·전송을 건너뛴다. 탭이 다시 보이는 시점에는
"마운트 유지" 절에서 설명하는 `active` prop 전환 effect가 `fit()` + `resize_session`을
명시적으로 다시 호출한다(`ResizeObserver`가 `display:none → 보임` 전환에서 안정적으로
발화한다는 보장에만 기대지 않는다).

## 탭 모델 — Windows Terminal 방식 (탭 안에 분할)

v0.2.0에서 만든 grid/cols/rows 분할(`App.tsx:6,18,65-71`)은 그대로 두되, 그 상태를
앱 전역이 아니라 탭 하나의 속성으로 옮긴다.

```ts
type Layout = "grid" | "cols" | "rows";

interface Pane {
  id: string;      // 세션 id (Rust start_session의 반환값)
  distro: string;
}

interface Tab {
  id: string;
  title: string;
  layout: Layout;
  paneIds: string[];   // panes 배열에 대한 참조. 소속 판단의 단일 진실 소스.
}
```

앱 상태(`App.tsx`): `panes: Pane[]`(세션 풀, 탭 소속 정보를 갖지 않는다),
`tabs: Tab[]`, `activeTabId: string`, 그리고 신규 `activePaneId: string | null`.
`activePaneId`는 지금 코드베이스에 대응 개념이 없다 — Ctrl+Shift+W("활성 팬 닫기")를
구현하려면 "지금 어떤 팬이 활성인가"를 알아야 하므로 새로 만든다. 팬을 클릭(마우스
다운)하면 갱신한다.

### 불변식: 탭은 항상 팬을 최소 1개 가진다

탭 생성과 그 탭의 첫 팬 생성을 한 동작으로 묶는다 — 세션 시작이 성공한 뒤에만 탭을
만든다(`startInTab(null, distro)` → 세션 시작 성공 → 탭 생성 + `paneIds: [id]`).
반대 방향도 마찬가지로, 팬을 닫아 탭의 `paneIds`가 빈 배열이 되면 그 탭도 함께
지운다. 그 결과 "팬이 0개인 탭"은 상태에 존재하지 않는다 — 별도의 빈 탭 렌더링 분기가
필요 없어지고, 탭 제목 규칙(뒤에서 설명)도 "탭 생성 시점에 distro를 안다"는 전제를
쓸 수 있다.

이 불변식 때문에 툴바의 "+ Terminal" 버튼과 `Ctrl+Shift+D`는 사실상 같은 함수를
호출한다: 활성 탭이 있으면 그 탭에 팬을 추가(분할)하고, 탭이 하나도 없으면(앱을 막
띄운 직후) 새 탭을 만든다. `Ctrl+Shift+T`만 "탭이 있어도 항상 새 탭을 만든다"로 분기가
다르다.

## ⚠ 최대 함정: 탭 전환 시 xterm 인스턴스를 언마운트하지 않는다

`TermPane`이 언마운트되면 cleanup에서 `term.dispose()`가 불린다(`TermPane.tsx:69`).
스크롤백은 xterm.js 내부 버퍼에만 있으므로 dispose되면 통째로 사라진다. 탭을
`{activeTabId === tab.id && tab.paneIds.map(...)}` 같은 조건부 렌더링으로 구현하면
탭을 오갈 때마다 모든 팬이 이 경로를 타 스크롤백이 날아간다 — 터미널 앱으로서
치명적이다.

### 채택한 해법: React Portal로 마운트 위치만 옮긴다

일반적인 "모든 탭의 grid를 항상 렌더링하고 `display:none`으로 숨긴다" 방식은 탭
전환에는 통하지만, **팬을 다른 탭으로 드래그 이동**할 때는 통하지 않는다 — 팬이
속한 그리드(부모 DOM)가 바뀌면 React는 그 컴포넌트를 다른 부모 아래로 옮긴 것으로
보고 여전히 언마운트 후 재마운트한다(자식이 트리에서 위치를 옮기면 재조정 대상이
아니라 제거+삽입으로 처리됨). 탭 전환과 팬 이동, 두 시나리오 모두를 한 메커니즘으로
풀기 위해 `ReactDOM.createPortal`을 쓴다.

- `PaneCanvas` 컴포넌트가 DOM 노드 2개를 갖는다: 활성 탭의 grid 컨테이너(`.panes`,
  콜백 ref로 상태에 저장)와 항상 화면 밖에 있는 holding pen(`.pane-holding`,
  `display:none`).
- `panes` 배열의 각 세션마다 `TermPane`을 정확히 한 번 렌더링하되, `createPortal(
  <TermPane .../>, target, paneId)`로 감싼다. `target`은 그 팬이 속한 탭이
  `activeTabId`와 같으면 grid 컨테이너, 다르면 holding pen이다.
- 탭을 전환하면 각 팬의 `target`이 바뀌지만 `<TermPane key={paneId}>` React 엘리먼트
  자체는 이전과 "같은" 자리(같은 `.map()` 호출 안, 같은 key)에서 계속 리턴되므로
  React는 재조정만 하고 언마운트하지 않는다 — portal은 정확히 "같은 컴포넌트 인스턴스를
  유지한 채 렌더링 위치만 바꾸는" 용도로 설계된 API라 이 문제에 들어맞는다.
- 팬을 다른 탭으로 드래그하면(뒤에서 설명) `tabs` 상태에서 `paneIds` 소속만 바뀌고,
  세션 자체는 Rust `SessionState`가 들고 있으므로 프론트는 어느 탭에 "보여줄지"만
  바꾸면 된다. 이 경우도 `target`이 바뀔 뿐 `TermPane` 엘리먼트는 유지된다.

`activeGridEl`/`holdingEl`은 `useState<HTMLDivElement | null>` + 콜백 ref로 관리한다
(첫 렌더에는 ref가 아직 null이라 어떤 팬도 portal되지 않지만, `.panes`/`.pane-holding`
div 자체는 항상 렌더링되므로 마운트 직후 ref가 채워지고 재렌더링되어 이후 생성되는
모든 팬에는 영향이 없다).

### 탭이 다시 보일 때

`TermPane`은 `active: boolean` prop(자신이 속한 탭이 `activeTabId`인지)을 받는다.
`active`가 바뀔 때 실행되는 별도 effect가 `fit()` 재계산 + `resize_session` 재전송을
명시적으로 수행한다 — `display:none`이던 holding pen에서 grid로 옮겨진 직후
`ResizeObserver`가 확실히 발화한다는 브라우저 스펙상의 보장이 약하므로(구현체마다
차이가 있었던 이력이 있다), 이 경로를 유일한 신호원으로 삼지 않는다.

## broadcast 범위 — 동작 변경, 근거를 남긴다

### 현재 동작과 왜 탭과 함께 버그가 되는가

`terminal.rs:178`:

```rust
#[tauri::command]
pub fn broadcast(state: tauri::State<'_, Arc<SessionState>>, data: String) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    for h in sessions.values() {
        // 등록된 "모든" 세션에 쓴다 — 탭 구분이 없다
        ...
```

지금은 세션이 전부 한 화면(하나의 grid)에 있으니 "모든 세션 = 화면에 보이는 모든
세션"이라 문제가 없었다. 탭이 생기면 이 등식이 깨진다 — 사용자가 탭 A에서 broadcast를
켠 채 명령을 치면, 화면에 보이지도 않는 탭 B·C의 셸에도 같은 입력이 들어간다. 다른
탭에서 `rm`이 실행 중이었다면 최악의 경우 데이터 손실로 이어질 수 있다.

### 변경

시그니처를 `broadcast(session_ids: Vec<String>, data: String)`으로 바꾸고, 호출부
(`terminal.rs:178-186`)는 등록된 전체 세션이 아니라 넘어온 id 목록만 순회한다.
프론트(`api.ts:30-33`)는 활성 탭의 `paneIds`만 넘긴다. `TermPane`은 렌더링 시점의
활성 탭 세션 id 목록을 `broadcastTargetIds` prop으로 받아 기존 `broadcastRef`와 같은
"매 렌더마다 ref 갱신" 패턴으로 최신 값을 유지한다(effect 의존성에 넣지 않는다 —
넣으면 broadcast 대상이 바뀔 때마다 xterm이 재마운트되어 앞 절의 함정을 다시 밟는다).

이건 기존 명령의 시그니처 변경이라 호출부가 하나뿐인데도(`TermPane.tsx`) 문서로
남긴다: **"모든 세션"에서 "활성 탭의 세션"으로 broadcast 범위를 좁히는 것이 의도된
동작 변경**이고, 이유는 탭 격리다.

## 드래그

### 탭 순서 변경

탭 바 안에서 HTML5 drag & drop 표준 API를 쓴다(`draggable`, `onDragStart`,
`onDragOver`, `onDrop`). 탭 pill을 드래그 시작하면
`dataTransfer.setData("application/x-wsld-tab", tab.id)`를 심고, 다른 탭 pill에
드롭하면 드래그한 탭을 드롭 대상 탭의 바로 앞으로 옮긴다(배열에서 제거 후
드롭 대상의 인덱스에 삽입).

### 팬을 다른 탭으로 이동

`TermPane.tsx:75`의 `.pane-head`를 `draggable`로 만들고, 드래그 시작 시
`dataTransfer.setData("application/x-wsld-pane", sessionId)`를 심는다. 탭 pill의
`onDrop`은 두 mime 타입(`x-wsld-tab`/`x-wsld-pane`)을 구분해 분기한다 — 탭이 오면
순서 변경, 팬 id가 오면 그 탭의 `paneIds`로 옮긴다(원래 탭에서 제거하고, 원래 탭의
`paneIds`가 비면 [불변식](#불변식-탭은-항상-팬을-최소-1개-가진다)에 따라 원래 탭도
닫는다). 옮긴 뒤 대상 탭을 활성화하고 `activePaneId`도 그 팬으로 옮긴다. 세션은
Rust `SessionState`가 계속 들고 있으므로 이동은 프론트 상태(`paneIds` 소속)만
바꾸는 일이고, `TermPane` 인스턴스는 (portal 덕에) 유지된다.

시각 피드백: 드롭 가능한 탭 pill 위에 드래그 중인 요소가 올라가면(`onDragOver`에서
`e.dataTransfer.types`를 확인해 우리 mime 타입일 때만) `.drag-over` 클래스를 붙여
테두리를 강조한다. `onDragLeave`에서 뗀다.

## 단축키 — Windows Terminal 호환

| 키 | 동작 |
|---|---|
| `Ctrl+Shift+T` | 새 탭 (`startInTab(null, selected)`) |
| `Ctrl+Shift+D` | 활성 탭에 팬 추가/분할 (`startInTab(activeTabId, selected)`) — 툴바 "+ Terminal"과 동일 함수 |
| `Ctrl+Shift+W` | 활성 팬 닫기 (`closePane(activePaneId)`, 마지막 팬이면 탭도 닫힘) |
| `Ctrl+Tab` | 다음 탭 |
| `Ctrl+Shift+Tab` | 이전 탭 |
| `Ctrl+Alt+1`~`9` | n번째 탭으로 이동 (범위를 벗어나면 무시) |

### 왜 bash 단독 바인딩(`Ctrl+W`/`Ctrl+T`)을 쓰지 않는가

`Ctrl+W`는 readline(bash 포함 거의 모든 셸)에서 "커서 앞 단어 삭제"이고, `Ctrl+T`는
readline 기본 바인딩인 "문자 위치 바꾸기(transpose-chars)"이자 많은 사용자 설정에서
fzf의 파일 검색 위젯으로 재바인딩되어 있다. 이 앱이 두 조합을 가로채면 셸 안에서
평소 쓰던 줄 편집이 조용히 죽는다 — 터미널 에뮬레이터로서는 받아들일 수 없는 트레이드
오프다. Windows Terminal 자신도 이 충돌 때문에 `Ctrl+Shift+*` 조합을 쓰므로 그대로
따른다.

### 구현: xterm이 키를 셸로 보내기 전에 가로챈다

xterm.js는 기본적으로 모든 keydown을 셸로 보낸다. `Terminal.attachCustomKeyEventHandler
(handler)`를 쓰면 xterm이 내부 처리를 하기 전에 먼저 `handler(event)`를 호출하고,
`false`를 반환하면 그 키를 더 이상 처리하지 않는다(셸로 데이터도 안 가고, xterm 내부
동작도 안 함). 이 핸들러는 keydown과 keyup 모두에 대해 불리므로 `event.type ===
"keydown"`일 때만 판단한다.

```ts
term.attachCustomKeyEventHandler((event) => {
  if (event.type !== "keydown") return true;
  const action = matchShortcut(event);
  if (!action) return true;
  event.preventDefault();
  event.stopPropagation();
  onShortcutRef.current(action);
  return false; // xterm이 이 키를 처리(=셸 전송)하지 않는다
});
```

`matchShortcut(event: KeyboardEvent): ShortcutAction | null`은 `src/lib/shortcuts.ts`의
순수 함수로, 위 표의 조합을 판별한다.

### 터미널 밖에 포커스가 있을 때: window 레벨 리스너

탭 바나 cwd 입력칸 등 터미널이 아닌 곳에 포커스가 있으면 xterm의
`attachCustomKeyEventHandler`는 애초에 호출되지 않는다(그 리스너는 xterm이 만든
숨은 textarea에 붙는다). 이를 위해 `App.tsx`에 window 레벨 `keydown` 리스너를 하나
더 둔다.

**두 경로가 중복 실행되지 않도록**: `TermPane`의 커스텀 핸들러가 단축키를 인식하면
`event.stopPropagation()`을 호출한다 — 이 시점에 이벤트는 xterm의 숨은 textarea에서
발생해 아직 window까지 버블링되지 않은 상태이므로, stopPropagation으로 그 버블링
자체를 막으면 window 리스너는 애초에 이 이벤트를 보지 못한다. 이것이 1차 방어선이다.
추가로 window 리스너 안에서도 `document.activeElement`가 `.term-wrap` 안에 있으면
무시하는 방어적 가드를 하나 더 둔다(2차 방어선, stopPropagation이 어떤 이유로든
안 먹는 경우를 대비).

## Open path 고정 — 핀 토글 + 최근 경로

### 현재 동작

`App.tsx:49-58`의 `openTerminal`은 세션을 연 뒤 `setCwd("")`로 입력칸을 즉시 비운다
(`App.tsx:54`). 같은 경로에 터미널을 여러 개 열려면 매번 다시 입력해야 한다.

### 변경

- 입력칸 옆에 핀 토글 버튼을 추가한다. 꺼져 있으면(기본값) 기존과 동일하게 연 뒤
  입력칸을 비운다. 켜져 있으면 입력칸을 비우지 않고, 그 값을 `localStorage`에 저장해
  앱을 재시작해도 유지한다.
- 최근 사용 경로 5개를 `localStorage`에 MRU(최근 사용 순, 중복 제거)로 유지하고,
  cwd `<input>`에 `<datalist>`를 연결해 고를 수 있게 한다. 핀 여부와 무관하게, 경로를
  입력해 터미널을 연 적이 있으면 항상 최근 목록에 반영한다.
- `apps/wsl-dashboard/src/App.tsx:6-13`이 프로젝트 경로를 `localStorage`(`wsld-projects`
  키, `JSON.parse(... ?? "[]")` 패턴)에 저장하는 선례를 그대로 따른다. wsl-desktop은
  이름 충돌을 피하려고 `wsl-desktop:` 접두사를 쓴다(`wsl-desktop:cwd-pinned`,
  `wsl-desktop:cwd-value`, `wsl-desktop:recent-paths`) — 두 앱은 Tauri 별도 앱이라
  webview origin이 달라 실제로는 저장소가 격리되지만, 코드를 읽을 때 헷갈리지 않도록
  구분한다.

## 탭 제목

v1 규칙: distro 이름을 그대로 쓰고, 이미 같은 이름(또는 `이름 N` 패턴)을 쓰는 탭이
있으면 다음 번호를 붙인다(`Ubuntu` → 다음은 `Ubuntu 2`). 앞서 정한 불변식(탭은 항상
자신의 첫 팬과 함께 생성됨) 덕분에, 탭을 만드는 시점에 distro가 이미 확정돼 있으므로
제목은 생성 시 한 번만 계산하고 이후 바뀌지 않는다. 사용자 편집은 범위 밖.

```ts
// src/lib/tabTitle.ts — 순수 함수, distro/기존 탭 제목 목록만 받는다
export function nextTabTitle(existingTitles: string[], distro: string): string {
  const pattern = new RegExp(`^${escapeRegExp(distro)}( \\d+)?$`);
  const count = existingTitles.filter((t) => pattern.test(t)).length;
  return count === 0 ? distro : `${distro} ${count + 1}`;
}
```

## 테스트에 대한 솔직한 서술

이 변경은 대부분 프론트 상태 관리(탭 배열 조작, portal 배선, DOM 이벤트 배선)이고,
`apps/wsl-desktop`을 포함해 이 저장소에는 프론트 테스트 인프라가 전혀 없다
(vitest/jest 0, `package.json`에 관련 devDependency 없음). 이번 작업에서도 도입하지
않는다(브리핑의 명시적 범위 밖 항목).

`terminal.rs`는 대부분 OS I/O(PTY open/read/write/resize, 프로세스 spawn)라 실제로
단위 테스트가 가능한 순수 로직은 원래부터 `parse_distros`(`terminal.rs:219-225`)와
`decode_output`(`terminal.rs:229-248`) 둘뿐이었고, 이번 변경(`resize_session`,
`broadcast` 시그니처 변경)도 둘 다 상태(`Mutex<HashMap<...>>`)를 직접 다루는 명령
레이어라 새로 테스트 가능한 순수 로직이 생기지 않는다. 억지로 만들지 않는다.

프론트의 새 순수 로직 — `matchShortcut`(`src/lib/shortcuts.ts`)과
`nextTabTitle`(`src/lib/tabTitle.ts`) — 은 원래대로라면 유닛 테스트에 가장 적합한
후보지만, 프론트 테스트 러너가 없는 채로 이것 하나만을 위해 vitest를 들이는 것은
범위를 벗어난다고 판단해 테스트 없이 둔다. 대신 아래 Windows 수동 검증 체크리스트에
단축키 5종과 탭 제목 규칙(동일 distro 중복 시 번호 증가)을 항목으로 명시했다.

Rust 쪽에 이번 변경으로 새 순수 로직이 생기지는 않았다. 만약 이후에 탭 제목 생성
규칙을 Rust로 옮기는 리팩터를 한다면(지금은 옮기지 않는다 — TS에 있는 편이 자연스럽고,
탭은 프론트 전용 개념이라 Rust가 알 필요가 없다) 그때 `#[cfg(test)] mod tests`를
붙인다.

### Windows 수동 검증 체크리스트

- [ ] 터미널 2개를 다른 탭에 열고, 탭 A에 텍스트를 충분히 출력(`ls -la /` 여러 번
      등)한 뒤 탭 B로 전환했다가 탭 A로 돌아온다 — 스크롤백이 그대로 남아 있다
      (사라지거나 리셋되지 않는다)
- [ ] 탭 A에서 `vim`을 열고 탭 B로 전환했다가 돌아온다 — 화면이 실제 패인 크기에
      맞게 다시 그려진다(30×100 고정 크기로 깨지지 않는다)
- [ ] 한 탭 안에서 팬 2개 이상을 열고 레이아웃을 grid → cols → rows로 바꾼다 —
      각 팬 안의 `htop`(또는 `vim`)이 매번 새 패인 크기에 맞게 다시 그려진다
- [ ] 탭 A, B를 열고 A에서 broadcast를 켠 뒤 A의 한 팬에 입력한다 — A의 다른 팬에는
      들어가고 B의 팬에는 들어가지 않는다
- [ ] `Ctrl+Shift+T`로 새 탭이 열리고 즉시 터미널이 실행된다
- [ ] `Ctrl+Shift+D`로 활성 탭에 팬이 분할 추가된다 (툴바 "+ Terminal"과 동일 결과)
- [ ] `Ctrl+Shift+W`로 활성 팬이 닫힌다. 그 팬이 탭의 마지막 팬이면 탭도 함께 닫히고
      다른 탭으로 포커스가 이동한다
- [ ] `Ctrl+Tab` / `Ctrl+Shift+Tab`으로 탭이 순서대로 다음/이전으로 전환된다
      (마지막 탭에서 `Ctrl+Tab`은 첫 탭으로 순환)
- [ ] `Ctrl+Alt+1`~`9`로 해당 번호 탭으로 바로 이동한다(탭이 그 수보다 적으면 무시)
- [ ] 터미널 안에서 `Ctrl+W`(단어 삭제)·`Ctrl+T`(fzf 등)가 여전히 셸에 그대로
      전달된다 — 이 앱이 가로채지 않는다
- [ ] cwd 입력칸에 경로를 입력하고 핀을 켠 뒤 터미널을 연다 — 입력칸이 비워지지
      않고, 앱을 재시작해도 같은 경로가 남아 있다
- [ ] 핀을 끄고 터미널을 연다 — 기존처럼 입력칸이 비워진다
- [ ] cwd 입력칸의 최근 경로 목록(datalist)에 최근 연 경로 최대 5개가 뜬다
- [ ] 탭 바에서 탭을 드래그해 순서를 바꾼다 — 드롭한 위치에 탭이 옮겨진다
- [ ] 한 탭의 팬 헤더를 다른 탭 위로 드래그해 놓는다 — 그 팬이 대상 탭으로
      옮겨지고, 원래 탭에 팬이 더 있었다면 원래 탭은 그대로 남는다(마지막 팬이었다면
      원래 탭도 함께 닫힌다). 이동 중 스크롤백이 유지된다
- [ ] 드래그 중 드롭 가능한 탭 위에 마우스를 올리면 시각적으로 강조된다
- [ ] 같은 distro로 탭을 여러 개 열면 제목이 `Ubuntu`, `Ubuntu 2`, `Ubuntu 3`처럼
      증가한다

## 완료 기준 (CI와 동일)

WSL에서:

```bash
source ~/.cargo/env
cargo test -p wsl-desktop
cargo clippy -p wsl-desktop --all-targets -- -D warnings
cargo fmt --all --check
cd apps/wsl-desktop && pnpm build
```

## 범위 밖

- 탭 제목 사용자 편집
- 팬 위치 드래그 재배치(같은 탭 안에서 팬끼리 자리 바꾸기) — 탭 간 이동만 지원
- 세션 영속화/복원(앱 재시작 시 탭·팬 복구)
- 탭별로 다른 distro 세트를 미리 구성하는 UI
- `apps/wsl-desktop` 외 다른 앱 수정
- 프론트 테스트 인프라(vitest 등) 도입
