# knowledge-base 마크다운 렌더링 + mermaid 프리뷰 — 설계

- 날짜: 2026-08-11
- 범위: `apps/knowledge-base`에 마크다운 렌더링과 mermaid 다이어그램 프리뷰 추가 (편집기 자체는 교체하지 않음)
- 산출: 이 설계 문서. 구현은 별도 PR(들)로 진행한다.

## 배경

knowledge-base는 마크다운 지식 저장소를 표방하지만 정작 마크다운을 렌더링하지 못한다.
편집기는 `<textarea>` 하나뿐이고(`apps/knowledge-base/src/App.tsx:211-225`), `readFile`로
읽은 원문을 그대로 보여주고 그대로 저장한다(`src/api.ts:24-34`, `src-tauri/src/commands/docs.rs:56-75`).
frontmatter는 검색 인덱싱에만 쓰인다 — `core::db::index_doc`이 `frontmatter::parse`로
title/tags를 뽑아 FTS5 인덱스에 넣을 뿐(`core/db.rs:59-75`, `parse` 정의는
`core/frontmatter.rs:16-61`), 화면에는 아무 가공 없이 원문 그대로 노출된다.

`PLAN.md`는 애초에 v1 목표로 "Markdown 편집: CodeMirror + 프리뷰 토글"(`PLAN.md:17`)과
`core/markdown.rs`(`PLAN.md:62`, "본문 추출, `[[링크]]` 감지")을 예정해뒀지만 실제 구현에서는
빠졌다. 이 설계는 그 구멍(렌더링)을 메우고, 노트 안에 mermaid 다이어그램을 그릴 수 있게
한다.

이 저장소는 프론트 테스트 인프라가 전혀 없다. `apps/knowledge-base/package.json`을 포함해
어떤 앱의 `package.json`에도 vitest/jest/testing-library류 러너가 없고, 프론트에 대한 CI
검증은 `pnpm build`(타입체크 + 번들)뿐이다(`AGENTS.md:17`, CONVENTIONS §5). 반대로 Rust
쪽은 `apps/`+`crates/` 전체에 `#[test]` 66개가 있고(`grep -rn '#\[test\]' apps crates | wc -l`
로 확인, 2026-08-11 기준) `cargo test`로 전부 검증된다. 렌더링 로직을 어디에 둘지가 이
설계의 첫 갈림길이며, 아래 결정 1은 그 갈림길에 대한 답이다.

## 결정과 근거

### 1. 렌더링은 Rust가 한다

`pulldown-cmark`로 마크다운을 HTML로 변환하고 `ammonia`로 살균한다. 프론트가 새로
얻는 의존성은 `mermaid` 하나뿐이다.

근거: 위 배경에서 확인했듯 이 저장소에는 프론트 테스트 인프라가 없다. 렌더링·살균·
mermaid 블록 추출 같은 로직을 TypeScript에 두면 CI가 그 로직을 검증할 방법이 `pnpm build`의
타입체크뿐이다 — 런타임 동작(HTML이 올바르게 만들어지는지, `<script>`가 걸러지는지,
mermaid 블록 인덱스가 맞는지)은 아무도 자동으로 확인하지 않는다. Rust에 두면 이 저장소의
검증 방식 그대로(`cargo test`) 66개 테스트와 같은 방식으로 커버된다.

### 2. 에디터는 `<textarea>`를 유지한다

CodeMirror 도입은 이 설계의 범위가 아니다. 후속 앱 code-pad가 CodeMirror를 먼저 도입하고,
그때 knowledge-base가 `packages/editor`의 **두 번째 소비자**가 되어 CONVENTIONS의
공통화 원칙대로("두 번 이상 실제로 필요해진 코드만 `packages/`·`crates/`로 추출한다.
처음부터 공용 패키지를 미리 만들지 않는다", `CONVENTIONS.md:17-19`) 뽑아낸다. 지금
knowledge-base 혼자를 위해 CodeMirror를 넣는 것은 이 원칙의 "첫 소비자는 앱 안에 코드를
둔다"는 절반만 지키고 공용화를 앞당기는 셈이 되어 원칙과 어긋난다.

### 3. 코드 위치는 앱 안 — `apps/knowledge-base/src-tauri/src/core/markdown.rs`

`crates/markdown`으로 선제 추출하지 않는다. 이유는 두 가지다.

- CONVENTIONS §4의 추출 기준: "같은 도메인 코드가 **두 번째 앱**에서 필요해지면
  `crates/<domain>/`로 옮긴다"(`CONVENTIONS.md:135-137`). 지금은 knowledge-base
  하나뿐이라 인터페이스가 한 소비자로만 검증된 상태다.
- code-pad는 요구사항 자체가 다르다: frontmatter 처리가 필요 없고, `.mmd` 단독 파일을
  다뤄야 하며(마크다운에 파묻힌 mermaid 블록이 아니라 파일 전체가 mermaid), 스크롤
  동기화가 필요해질 수 있다. 지금 추출하면 만들어보지 않은 요구사항에 맞춰 인터페이스를
  일반화하게 된다.

code-pad 차례가 오면 `apps/knowledge-base/src-tauri/src/core/markdown.rs`를
`crates/markdown/`으로 옮기고 루트 `Cargo.toml`의 `workspace.members`에 추가하는,
이 저장소에서 이미 반복된 패턴(`Cargo.toml:5-15`에 앱마다 `src-tauri`가 추가되어 온 것과
동일한 절차)을 따른다.

### 4. 프리뷰는 3모드: 편집 / 분할 / 프리뷰

툴바 버튼으로 전환한다. 모드 상태는 앱(React) 수준에서만 관리하고 디스크에 저장하지
않는다 — 문서별 설정이 아니라 세션 동안의 UI 취향이기 때문이다. `core::db`의
`settings` 테이블(`core/db.rs:13-16`)에 넣을 만큼 영속화할 가치가 없다.

### 5. 렌더 범위

포함: 마크다운 본문, mermaid, 링크(내부/외부), 로컬 이미지.
제외: 코드블록 문법 하이라이팅, 위키링크 `[[...]]`, 백링크, 스크롤 동기화. (근거는
"범위 밖" 절 참고. `PLAN.md:62`가 애초에 예정했던 `[[링크]]` 감지도 여기서 명시적으로
제외한다 — v2 백링크 기능(`PLAN.md:23`)이 아직 없는데 렌더러가 먼저 위키링크 문법을
해석하면 가리키는 대상이 없는 링크를 보여주게 된다.)

### 6. mermaid 오류 처리 — 마지막 성공 SVG 유지

사용자가 타이핑하는 동안 mermaid 문법은 거의 항상 일시적으로 깨진 상태를 거친다
(여는 괄호만 치고 닫는 괄호를 아직 안 쳤다든지). 렌더 실패마다 다이어그램을 지우고
빈 자리나 에러 메시지로 덮으면 편집 경험이 나빠진다. 대신:

- 프론트가 `Map<blockIndex, lastGoodSvg>`를 들고 있는다.
- mermaid 렌더가 성공하면 해당 인덱스의 SVG를 갱신한다.
- 실패하면 그 인덱스에 저장된 마지막 성공 SVG를 그대로 유지하고, 구석에 작은
  "구문 오류" 배지만 띄운다.

**한계(수용함)**: 블록을 추가·삭제하면 이후 블록들의 인덱스가 밀린다. 예를 들어
mermaid 블록이 2개(인덱스 0, 1)인 문서에서 맨 앞에 새 mermaid 블록을 끼워 넣으면
기존 인덱스 0, 1이었던 블록이 1, 2가 되고, `Map`에는 옛 인덱스 기준 SVG가 남아 있어
편집 중 잠깐 엉뚱한 블록에 엉뚱한 SVG가 붙어 보일 수 있다. 디바운스 렌더가 곧 새
결과로 덮어쓰므로 짧은 시각적 결함으로 끝나지만, 구조적으로 존재하는 한계이므로
없애려 하지 않고 문서에 남긴다.

### 7. 이미지는 Rust가 `data:` URI로 인라인한다

Tauri의 asset protocol을 쓰지 않는다. `tauri.conf.json`에는 현재
`app.security.assetProtocol` scope 설정이 아예 없다(`src-tauri/tauri.conf.json` 전체
확인). Knowledge 루트는 고정 경로가 아니라 사용자가 런타임에 바꾸는 설정값이다 —
`set_root` 커맨드가 `db::set_setting(conn, "root", &path)`로 바꾸고(`commands/docs.rs:38-43`),
`resolve_root`가 매 요청마다 그 값을 DB에서 읽는다(`commands/docs.rs:20-30`). asset
protocol의 scope는 `tauri.conf.json`에 정적으로 선언하거나 런타임에 별도 API로
조작해야 하는데, 사용자가 루트를 바꿀 때마다 scope를 갱신하는 코드를 추가하는 것보다
기존에 이미 있는 `safe_join` 기반 검증 경로(`core/store.rs:8-17`)를 이미지에도 그대로
재사용하는 편이 코드 경로가 하나 줄고 기존 보안 검증(경로 탈출 차단)을 그대로
물려받는다.

크기 상한 2MB. 초과·부재·루트 밖이면 대체 표시로 이유를 보여준다.

## 인터페이스

```rust
#[tauri::command]
pub fn render_markdown(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,      // 현재 문서의 루트 상대 경로 (이미지·링크 해석 기준)
    content: String,  // 저장 전 편집 중 내용
) -> Result<RenderedDoc, String>

pub struct RenderedDoc {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub html: String,          // 살균 완료, mermaid는 placeholder로 치환됨
    pub mermaid: Vec<String>,  // placeholder 인덱스 순서의 mermaid 원문
}
```

`content`를 파라미터로 받는 이유는 기존 `write_file` 커맨드와 같은 이유다 — 저장 전
편집 중인 내용을 렌더해야 하므로 디스크에서 다시 읽지 않는다(`commands/docs.rs:64-75`의
`write_file`도 프론트가 들고 있는 `content`를 그대로 받는다).

`state`가 필요한 이유는 루트 경로를 알아야 이미지를 `safe_join`으로 검증하고 읽을 수
있어서다. 기존 `read_file`/`write_file`과 동일하게 `state.db.lock()` →
`resolve_root(&conn)` → 루트 절대경로를 얻는 절차를 그대로 밟는다(`commands/docs.rs:56-62`
패턴).

## 모듈 구조 — OS 의존 분리

CONVENTIONS §4의 "core는 OS 의존 없음, WSL에서 cargo test" 원칙(`CONVENTIONS.md:131`,
"`core/` 앱 로컬 순수 로직 — OS 의존 없음, WSL에서 cargo test")을 지키기 위해
**이미지 로더를 주입**받는 순수 함수로 만든다.

```rust
pub fn render(body: &str, load_image: &dyn Fn(&str) -> ImageResult) -> (String, Vec<String>)
```

테스트는 가짜 로더를 주입해 파일시스템 없이 검증한다. 실제 로더는 command 레이어가
만든다: `safe_join`(경로 검증) + 크기 검사 + `fs::read` + 확장자→MIME 추론 + base64
인코딩. 이 4단계는 모두 IO/OS 의존이므로 `core/markdown.rs`가 아니라
`commands/docs.rs`(또는 신설 command 파일)에 둔다 — 정확히 `core/store.rs`의
`safe_join`이 순수 함수(경로 계산만, IO 없음)이고 `read_file`/`write_file`은 IO를 하는
별도 함수로 나뉘어 있는 것과 같은 분리다(`core/store.rs:8-17` vs `:45-54`).

변경/신규 파일 목록:

| 파일 | 변경 |
|---|---|
| `apps/knowledge-base/src-tauri/src/core/markdown.rs` | 신규. `render()` 순수 함수 + `ImageResult` + 9개 테스트 |
| `apps/knowledge-base/src-tauri/src/core/mod.rs` | `pub mod markdown;` 추가 (현재 `db`/`frontmatter`/`store` 3줄, `core/mod.rs:1-3`) |
| `apps/knowledge-base/src-tauri/src/commands/docs.rs` | `render_markdown` 커맨드 추가. 실제 이미지 로더(클로저) 구현, `frontmatter::parse` 재사용(`core/frontmatter.rs:16`) |
| `apps/knowledge-base/src-tauri/src/lib.rs` | `invoke_handler![...]` 목록에 `commands::docs::render_markdown` 추가 (`lib.rs:12-24`) |
| `apps/knowledge-base/src-tauri/Cargo.toml` | `[dependencies]`에 `pulldown-cmark`, `ammonia`, `base64` 추가 (현재 `:20-26`) |
| `apps/knowledge-base/src/types.ts` | `RenderedDoc` 인터페이스 추가 (현재 `TreeEntry`/`SearchResult`만 있음, `types.ts:1-9`) |
| `apps/knowledge-base/src/api.ts` | `renderMarkdown(rel, content)` invoke 래퍼 추가, 기존 패턴대로 `isTauri()` mock 분기 포함 (`api.ts:1-66`의 다른 함수들과 동일한 모양) |
| `apps/knowledge-base/src/App.tsx` | 3모드 상태, 300ms 디바운스, 프리뷰 컨테이너(이벤트 위임 + mermaid `Map` 캐시), 툴바 버튼 추가 |
| `apps/knowledge-base/package.json` | `dependencies`에 `mermaid@^11.16.1` 추가 (현재 `:12-17`) |

프론트 쪽은 현재 `App.tsx` 하나에 트리·편집기·검색이 모두 들어 있는 단일 파일
구조다. CONVENTIONS §4가 권장하는 `src/components/`(`CONVENTIONS.md:148`)를 아직
어떤 앱도 실제로 쓰고 있지 않으므로, 프리뷰 컨테이너를 `App.tsx` 안에 둘지
`src/components/`로 분리할지는 이 설계에서 강제하지 않는다 — 둘 다 CONVENTIONS와
충돌하지 않는다.

## 데이터 흐름

1. 사용자가 `<textarea>`에 입력 → `setContent` + `setDirty(true)` (기존 로직,
   `App.tsx:214-217`).
2. 모드가 분할/프리뷰이고 선택된 파일이 `.md`로 끝나면, 입력마다 300ms 디바운스
   타이머를 리셋한다.
3. 타이머가 만료되면 `invoke('render_markdown', { rel: selected, content })` 호출
   (camelCase 변환은 기존 `invoke()` 래퍼들과 동일, `api.ts` 패턴).
4. 커맨드 핸들러: `state.db.lock()` → `resolve_root(&conn)`으로 루트 절대경로 획득
   (`commands/docs.rs:20-30`, 기존 `read_file`/`write_file`과 동일 절차).
5. `frontmatter::parse(&content)`로 `(DocMeta, body)` 분리 (`core/frontmatter.rs:16-61`,
   `index_doc`이 인덱싱에 쓰는 것과 같은 함수).
6. 이미지 로더 클로저를 만든다: `rel`의 디렉터리 부분을 기준으로 상대 경로를 해석 →
   `store::safe_join(&root, ...)`으로 검증(`core/store.rs:8-17`) → 2MB 이하면
   `fs::read` + 확장자별 MIME 추론 + base64 인코딩.
7. `core::markdown::render(body, &load_image)` 호출. 내부에서 pulldown-cmark의
   이벤트 스트림을 순회하며:
   - `Tag::Image`의 `dest_url`을 로더 결과(성공 시 `data:` URI, 실패 시 대체 표시)로
     치환한다.
   - fenced code block 언어가 `mermaid`인 구간은 원문을 추출해
     `<div class="mermaid-block" data-idx="N"></div>`로 치환하고, 원문을
     `Vec<String>`에 순서대로 쌓는다. 언어가 다른 코드블록(예: ` ```rust `)은 건드리지
     않고 그대로 `push_html`로 넘긴다.
8. 만들어진 HTML을 `ammonia`로 살균한다. `div`/`class`/`data-idx`를 허용 목록에
   추가하고, 상대 경로 href가 살아남도록 URL 상대경로 정책을 통과로 설정한다.
   `javascript:` 같은 스킴은 ammonia 기본 동작으로 제거된다. 이미지 `data:` URI는
   `img[src]`에 한해 명시적으로 허용해야 한다 — ammonia의 기본 허용 URL 스킴에
   `data:`가 없기 때문에, 이 설정을 빠뜨리면 6~7단계에서 인라인한 이미지가 살균
   단계에서 통째로 잘려 나간다.
9. `RenderedDoc { title, tags, html, mermaid }`을 프론트로 반환.
10. 프론트: `title`/`tags`를 프리뷰 상단 헤더로 보여준다(frontmatter가 프리뷰 본문에
    날것으로 새지 않는 이유가 이것 — 5단계에서 이미 body와 분리했다). `html`을 프리뷰
    컨테이너에 삽입한다.
11. 컨테이너 안의 `.mermaid-block[data-idx]` 요소마다 `mermaid`를 호출해 SVG로
    렌더한다. 성공하면 `Map<blockIndex, svg>`를 갱신하고 DOM에 반영, 실패하면 해당
    인덱스의 기존 SVG를 유지한 채 에러 배지만 띄운다(결정 6).
12. 프리뷰 컨테이너 루트에 이벤트 위임 클릭 리스너 하나를 단다. `a[href]` 클릭을
    가로채 `http`/`https`면 `tauri-plugin-opener`로 기본 브라우저에 연다(이미
    의존성에 있음, `package.json:16`). 그 외 상대 경로는 현재 문서 디렉터리 기준으로
    해석해 `App.tsx`에 이미 있는 `openFile()`(`App.tsx:44-55`)을 호출한다 — 새 함수를
    만들지 않고 기존 파일 열기 경로를 재사용한다.

## 에러 처리

| 상황 | 처리 |
|---|---|
| mermaid 블록 렌더 실패(문법 오류, 대개 타이핑 중) | 해당 인덱스의 마지막 성공 SVG 유지 + "구문 오류" 배지. 인덱스가 밀리는 경우의 한계는 결정 6에서 명시하고 수용 |
| 이미지 부재 / 크기 2MB 초과 / 루트 밖 상대경로 | 대체 표시(이유가 보이는 placeholder). 이미지 하나의 실패가 전체 렌더를 실패시키지 않는다 — `render()`는 `Result`가 아니라 항상 `(String, Vec<String>)`을 반환하므로 이미지 로더 실패는 그 이미지 자리만 대체 표시로 바뀔 뿐 나머지 문서 렌더에 영향을 주지 않는다 |
| `render_markdown` 자체 실패(예: DB 락, 루트 미설정) | `Result<RenderedDoc, String>`의 `Err`로 전파. 프론트는 기존 에러 배너 패턴(`App.tsx:28`의 `error` state, `:137`의 렌더)을 그대로 재사용해 표시 |
| `.md`가 아닌 파일 | `render_markdown`을 호출하지 않는다 — 분할/프리뷰 버튼을 비활성화하고 편집 모드로 고정 |
| 문서 전환 중 인플라이트 렌더 응답 | "확정된 세부 결정" 3번 참고 — 요청 시점 `rel`을 캡처해 응답 시점에 현재 선택 문서와 비교, 다르면 버린다 |

## 테스트

전부 `apps/knowledge-base/src-tauri/src/core/markdown.rs`의 `#[cfg(test)] mod tests`에
둔다. `core/frontmatter.rs:64-97`, `core/store.rs:72-109`와 같은 위치·형태의 순수 함수
테스트이며, 가짜 이미지 로더를 주입하므로 파일시스템 없이 `cargo test`로 돈다.

1. frontmatter가 분리되어 body만 렌더된다
2. mermaid 블록이 placeholder로 치환되고 원문이 순서대로 `Vec<String>`에 반환된다
3. mermaid 블록이 여러 개일 때 `data-idx`가 0, 1, 2…로 매겨진다
4. `<script>`가 살균된다
5. 상대 링크 href가 유지된다
6. `javascript:` href가 제거된다
7. 이미지 로더가 실패를 반환하면 대체 표시가 들어간다
8. 이미지 로더가 성공하면 `src`에 `data:` URI가 들어간다
9. 일반 코드블록(예: ` ```rust `)은 mermaid로 취급되지 않는다

## 범위 밖

- 위키링크 `[[...]]` 감지, 백링크 패널 — `PLAN.md:23`의 v2 기능. 대상이 아직 없는
  링크를 렌더러가 먼저 해석하게 되는 순서 문제도 있다(결정 5 참고)
- 코드블록 문법 하이라이팅
- 프리뷰-편집기 스크롤 동기화
- CodeMirror 도입 (결정 2 — code-pad 차례)
- PlantUML 등 mermaid 이외의 다이어그램 문법
- PDF/HTML 내보내기
- 이미지 asset protocol 전환 (결정 7 — 근거는 유지되는 한 재검토 대상 아님)

## 확정된 세부 결정

이 설계 브리핑에는 없었지만 구현 전 확정된 항목들이다.

1. **`render_markdown`의 위치 → 신규 `commands/markdown.rs`로 분리한다.**

   근거: `commands/docs.rs`가 이미 200줄이고 파일 저장소 CRUD가 관심사다. 마크다운
   렌더링은 별개 관심사이고, CONVENTIONS §4가 규정한 `commands/<feature>.rs` 구조에도
   맞는다. `resolve_root`의 가시성을 `pub(crate)`로 넓혀 두 파일이 공유한다.

2. **`.md` 확장자를 서버에서 검증하지 않는다.**

   근거: 프론트가 분할·프리뷰 버튼을 비활성화하는 것으로 충분하다. Rust가 임의
   텍스트를 마크다운으로 렌더하는 것 자체는 무해하고, 나중에 `.mmd`나 `.markdown`을
   지원할 때 불필요한 장벽이 된다.

3. **인플라이트 렌더 응답 race를 처리한다.**

   렌더를 요청할 때의 `rel`을 캡처해두고, 응답이 도착하면 그 시점의 선택 문서와
   비교해 다르면 결과를 버린다. React에서는 최신 선택값을 `useRef`로 추적해
   비교한다. 근거: 문서를 빠르게 전환하면 늦게 도착한 이전 문서의 렌더 결과가 새
   문서의 프리뷰를 덮어쓴다.

4. **이미지 인코딩을 캐싱한다.**

   `AppState`에 `Mutex<HashMap<PathBuf, (SystemTime, String)>>` 캐시를 두고
   `(경로, mtime)`이 같으면 재사용한다. 항목 수 상한 32개, 초과하면 캐시를 통째로
   비운다(LRU까지 갈 필요 없다). 근거: 300ms 디바운스마다 최대 2MB 파일을 다시
   읽고 base64로 다시 인코딩하는 것은 실낭비다. 상한은 캐시가 무한히 자라지 않게
   하는 최소 장치다.

5. **`ImageResult`를 실패 사유별로 구분한다.**

   ```rust
   pub enum ImageResult {
       Inlined(String),   // data URI
       Passthrough,       // http/https — 원본 src를 그대로 둔다
       NotFound,
       TooLarge,
       OutsideRoot,
   }
   ```

   근거: 이미지가 안 보일 때 사용자가 이유를 알아야 고칠 수 있다. 대체 표시
   문구가 사유마다 달라야 한다.

## 구현 순서

각 단계가 이전 단계 없이도 `cargo test`/`cargo check`/`pnpm build`로 독립 검증되도록
순서를 잡는다(AGENTS.md의 완료 정의: `cargo test` + `cargo check` + `pnpm build`
통과).

1. `src-tauri/Cargo.toml`에 `pulldown-cmark`/`ammonia`/`base64` 추가, `cargo check`로
   컴파일만 확인 (로직 없음)
2. `core/markdown.rs`에 frontmatter 분리 + 순수 마크다운→HTML 변환(mermaid/이미지
   없이) 골격 작성 + 테스트 1 → `cargo test`
3. mermaid placeholder 치환 로직 + 테스트 2, 3, 9 (여러 블록 인덱싱, 일반
   코드블록과 구분) → `cargo test`
4. ammonia 살균 통합 + 테스트 4, 5, 6 (script 제거, 상대링크 유지, `javascript:`
   제거) → `cargo test`
5. 이미지 로더 주입 인터페이스(`&dyn Fn(&str) -> ImageResult`) + 가짜 로더로 테스트
   7, 8 → `cargo test` (파일시스템 불필요)
6. `render_markdown` 커맨드 추가: `safe_join` 기반 실제 이미지 로더 연결,
   `lib.rs`의 `invoke_handler!`에 등록 → `cargo check`
7. 프론트: `types.ts`에 `RenderedDoc`, `api.ts`에 `renderMarkdown()` 래퍼(mock 분기
   포함) 추가 → `pnpm build`
8. 프론트: `mermaid` 의존성 추가, 3모드 토글 UI, 프리뷰 컨테이너(이벤트 위임 +
   `Map<blockIndex, svg>` 캐시), 300ms 디바운스 배선 → `pnpm build`
9. Windows에서 `pnpm tauri dev`로 수동 검증: 실제 이미지 표시, mermaid 렌더,
   링크 클릭 시 오프너/`openFile` 분기, 타이핑 중 mermaid 오류 배지 동작 확인
   (WSL에서는 `pnpm tauri dev`를 돌릴 수 없으므로 이 단계만 Windows 필요,
   CONVENTIONS §1)
