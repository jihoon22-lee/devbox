# code-pad — 경량 코드 에디터 설계

- 날짜: 2026-08-12
- 브랜치: `docs/code-pad/lsp-phase2`
- 범위: 신규 앱 `apps/code-pad`의 설계. **이 문서 자체가 산출물이며 구현은 하지 않는다.**
- 전제: `crates/filesystem`의 현재 무제한 `collect` API는 PR #48로 이미 머지됐다.
  code-pad 구현보다 별도 `crates/filesystem` 제한 순회 후속 PR과 `crates/markdown`
  추출 PR이 먼저 머지되어야 한다. 근거는 "공통 추출"과 "구현 순서" 절에 있다.

## 배경

devbox 10개 앱 중 텍스트를 편집하는 앱은 knowledge-base뿐이고, 그 편집기는 `<textarea>`
하나다(`apps/knowledge-base/src/App.tsx` — `readFile`/`writeFile`을 감싸는 상태만 있고
CodeMirror 등 코드 에디터 라이브러리는 없다). 이 선택은 우연이 아니라 명시적 유보였다 —
knowledge-base의 마크다운 프리뷰 설계 문서가 "CodeMirror 도입은 이 설계의 범위가 아니다.
후속 앱 code-pad가 CodeMirror를 먼저 도입하고, 그때 knowledge-base가 `packages/editor`의
두 번째 소비자가 되어 ... 뽑아낸다"라고 못박아 두었다
(`docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:41-48`).
code-pad는 그 유보를 실행하는 앱이다.

CONVENTIONS는 프론트 공통 스택에 이미 "편집기: `@uiw/react-codemirror`"를 적어 두었다
(`CONVENTIONS.md:103`)지만, 저장소 전체에서 `apps/*/package.json`을 확인하면
`codemirror` 계열 의존성을 쓰는 앱이 하나도 없다 — code-pad가 이 스택 항목의 실제
첫 채택자다.

파일 트리·파일 검색은 이미 다른 두 앱이 각자의 방식으로 갖고 있다: knowledge-base는
`listTree`/`TreeEntry`로 노트 저장소 트리를 그리고(`apps/knowledge-base/src/api.ts`,
`apps/knowledge-base/src/types.ts`), everything-plus는 이름 그대로 로컬 파일 검색이
본업이다(`apps/everything-plus/src-tauri/Cargo.toml:4`, `description = "Everything+
Local Search"`). code-pad가 세 번째 파일 트리를 또 만드는 것은 이미 있는 두 개의
기능을 부분적으로 재구현하는 셈이 된다. 이 판단이 아래 "결정과 근거" 1번의
근거다.

### 목적

Notepad++를 대체할 가벼운 코드 에디터. 기반은 CodeMirror 6이다.

### 범위

**Phase 1**: 문법 하이라이팅, 탭, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크,
줄바꿈(CRLF/LF) 감지·변환, 인코딩 감지·변환, 큰 파일 가드, 단어 기반 자동완성,
프리뷰 패널(`.md`/`.mmd`), **단일 파일 내 찾기/바꾸기(정규식 포함)**. 마지막
항목은 최초 브리핑의 기능 목록·제외 목록 어디에도 없던 설계 누락이었다 — 근거는
"확정된 세부 결정" 6번.

**Phase 2**: 언어 중립 LSP 클라이언트와 Windows 로컬 stdio 서버 관리. 초기 카탈로그는
Rust/rust-analyzer, TypeScript·JavaScript/typescript-language-server,
Python/basedpyright, JSON·HTML·CSS/vscode-langservers-extracted를 제공한다. 서버
바이너리는 번들하지 않고, 사용자가 명시적으로 선택한 고정 버전만 검증·설치한다.
LSP 공통 구현은 아직 두 번째 실제 소비자가 없으므로
`apps/code-pad/src-tauri/src/lsp/`에 둔다. 두 번째 앱이 실제로 같은 구현을 소비할
때만 `crates/lsp`로 추출한다. 상세 인터페이스와 Phase 2 구현 순서는 아래
"Phase 2 — LSP" 절에서 정한다.

**제외**: 매크로, PDF, 바이너리/hex 편집, 터미널 내장, git 통합, 파일 트리 사이드바.

## 결정과 근거

### 1. 파일 트리를 두지 않는다 — 작업 폴더 1개 + Ctrl+P

배경에서 확인했듯 knowledge-base가 파일 트리를, everything-plus가 파일 검색을 이미
갖고 있다. code-pad는 작업 폴더 1개를 열어두고 `Ctrl+P` 빠른 열기로 그 안의 파일에
접근한다. 트리 사이드바 없이도 Notepad++가 원래 제공하지 않던 기능이므로 대체 목적에
부합한다(제외 목록에 "파일 트리 사이드바"가 명시된 이유이기도 하다).

### 2. 탭·분할 — 뷰 2개 고정, Notepad++ 모델

뷰 2개를 고정하고 각각 자기 탭 바를 가지며, 문서를 뷰 사이로 이동할 수 있게 한다.
상태 모양은 **전역 `docs: Doc[]` + `views: [string[], string[]]` + `activeView` +
뷰별 활성 문서**다. `docs`에는 각 문서가 한 번만 들어가며 생성 순서를 유지하고, 두
뷰의 배열에는 문서 ID만 넣는다. 문서를 옮길 때 바뀌는 것은 두 ID 배열의 소속뿐이고
전역 `docs` 배열은 재정렬하지 않는다. 이 불변식이 아래의 안정된 부모 렌더링과 undo
히스토리 보존을 함께 보장한다. Notepad++ 사용자가 옮겨올 때 그대로 아는 모델이라
학습 비용이 없고, 3개 이상으로 늘리는 임의 분할(VS Code류)보다 상태 전이 경우의 수가
훨씬 적어 "뷰 사이 문서 이동" 버그 표면을 좁힌다.

### 3. 세션 복원 범위 — 열린 파일·뷰 배치·커서 위치만, 미저장 내용은 제외

복원 대상은 열린 파일 경로, 뷰 배치, 커서 위치다. **미저장 버퍼의 내용은 복원하지
않는다.** 편집 중이던 텍스트(디스크에 없는 변경분)를 살리려면 버퍼 자체를 별도로
직렬화해 저장해야 하는데, 이는 세션 복원과 독립적인 별개의 큰 기능이다(자동 저장
주기, 충돌하는 디스크 변경과의 병합 등 새로운 문제가 따라온다). Phase 1에서는 열지
않는다.

### 4. 외부 변경 처리 — VS Code 방식

디스크에서 파일이 바뀌었을 때, 그 문서가 편집되지 않은 상태면 자동으로 다시 읽고,
편집 중이면 배너로 사용자에게 선택지를 준다(리로드 vs 유지). 자동 리로드만 하면
편집 중인 내용을 조용히 날리고, 항상 배너만 띄우면 건드리지도 않은 파일마저 매번
사용자 개입을 요구한다 — VS Code가 이미 검증한 절충이다.

## 공통 추출

CONVENTIONS의 공통화 원칙 — "두 번 이상 실제로 필요해진 코드만 `packages/`·`crates/`로
추출한다. 처음부터 공용 패키지를 미리 만들지 않는다"(`CONVENTIONS.md:17-19`) — 를
세 후보에 기계적으로 적용한 결과가 아래 표다.

| 대상 | 첫 소비자 | code-pad의 쓰임 | 판정 |
|---|---|---|---|
| `crates/filesystem` | everything-plus (사용 중) | Ctrl+P 폴더 순회 | **이미 추출됨 (#48)** — 현재 API는 무제한, 제한 순회 후속 PR 필요 |
| `crates/markdown` | knowledge-base (사용 중) | 프리뷰 패널 | **미래 추출** — code-pad 전에 별도 PR로 머지 |
| `packages/editor` | code-pad (최초) | — | **추출 안 함** — 첫 소비자 |

근거를 코드로 확인한 내용:

- **`crates/filesystem`**: PR #48이 everything-plus의 walk+ignore 로직을 공용
  크레이트로 옮겼다. `crates/filesystem/src/lib.rs:17-21`이 `collect`와
  `IndexedFile`을 re-export하고, `crates/filesystem/src/walk.rs:13-39`의 현재
  API는 `collect(root: &Path) -> Vec<IndexedFile>` 하나뿐이다. 이 구현은
  `WalkDir` 순회 중 제한이나 `truncated` 반환 없이 전체 파일을 수집한다.
  everything-plus는 `apps/everything-plus/src-tauri/Cargo.toml:27`의 path 의존과
  `commands/indexing.rs:7,116-118`의 `collect` 호출을 통해 이 무제한 동작을 계속
  사용한다. 제외 규칙은 `crates/filesystem/src/ignore.rs:1-24`의 18개 이름 매치다.
  code-pad의 Ctrl+P에는 제한 순회가 필요하지만 현재 API에는 그 기능이 없다. 따라서
  code-pad 구현 전에 별도 후속 PR(브랜치 `feat/crates/filesystem-limited-walk`)을
  둔다. 이 PR은 기존 `collect(root) -> Vec<IndexedFile>`를 그대로 유지하면서
  `collect_limited(root, max_entries) -> LimitedCollect` 호환 API를 추가한다.
  `LimitedCollect`는 `files: Vec<IndexedFile>`와 `truncated: bool`를 가지며, 제한에
  도달한 Rust 순회와 `truncated` 의미를 테스트한다.
  everything-plus는 계속 기존 `collect`를 호출하고, code-pad만 후속 PR이 머지된
  뒤 제한 API를 사용한다. 기존 `IndexedFile` 구조체의 필드는
  `crates/filesystem/src/walk.rs:5-11`에 정의돼 있다.
- **watcher는 옮기지 않는다**: everything-plus의 `Cargo.toml:25`에 `notify = "8.2.0"`이
  선언돼 있지만, `apps/everything-plus/src-tauri/src/` 전체에서 `notify::`를 참조하는
  코드가 **0건**이다(확인: `grep -rn "notify::" apps/everything-plus/src-tauri/src`).
  즉 notify/watcher는 실제 소비자가 code-pad 하나뿐이라 추출 기준(두 번째 소비자)을
  만족하지 못한다. `watcher.rs`는 code-pad 안에 남는다.
- **`crates/markdown`**: knowledge-base 설계 문서의 결정 3이 이미 이 판단을 내려
  뒀다 — "`crates/markdown`으로 선제 추출하지 않는다 ... code-pad 차례가 오면
  `apps/knowledge-base/src-tauri/src/core/markdown.rs`를 `crates/markdown/`으로
  옮기고 루트 `Cargo.toml`의 `workspace.members`에 추가하는 ... 절차를 따른다"
  (`docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:50-65`).
  code-pad가 프리뷰 패널에서 이 렌더러를 쓰게 되는 것이 그 "차례"다. 현재 파일의
  테스트 첫 항목은 `crate::core::frontmatter`를 가져오므로
  (`apps/knowledge-base/src-tauri/src/core/markdown.rs:188-203`), generic crate로
  옮길 때는 frontmatter가 이미 제거된 body를 직접 넘기는 테스트로 바꾼다.
  frontmatter 제거와 이미지 경로 해석은 각 앱의 command 레이어 책임으로 남긴다.
  이 `crates/markdown` 추출은 아직 미래의 별도 prerequisite이며, 그 PR이 머지되기
  전에는 code-pad가 crate를 의존하지 않는다.
- **`packages/editor`는 추출하지 않는다**: code-pad가 CodeMirror의 첫 소비자다(배경
  절 참고). CONVENTIONS §4의 추출 기준 문구 그대로 "같은 도메인 코드가 **두 번째
  앱**에서 필요해지면"(`CONVENTIONS.md:137-139`) 옮기므로, 첫 소비자 단계에서는
  `apps/code-pad/src/editor/`에 코드를 둔다. knowledge-base가 나중에 CodeMirror로
  옮겨 오는 시점이 두 번째 소비자다.

`crates/filesystem`은 PR #48로 이미 생성돼 루트 `Cargo.toml:16`의 workspace member가
됐고, 현재는 위에서 확인한 무제한 `collect` API를 제공한다. 반면 `crates/markdown`은
아직 디렉터리와 workspace member가 없으며, code-pad 구현 전에 별도 추출 PR로
추가할 미래 prerequisite다. CONVENTIONS §2의 저장소 구조 스케치(`CONVENTIONS.md:61-67`)
는 `crates/filesystem`의 예상 소비자로 "everything-plus, knowledge-base, life-log"를
적어 뒀지만, 이는 초기 설계 시점의 가상 표이고 현재 구현 사실을 대신하지 않는다.
실제 순회 API와 추출 시점은 현재 코드와 각 prerequisite PR의 계약을 기준으로 하며,
`packages/`는 이번에도 비어 있게 된다.

## 모듈 구조

CONVENTIONS §4의 앱별 Rust 모듈 구조(`CONVENTIONS.md:126-140`, `core/`는 OS 비의존
순수 로직·WSL에서 `cargo test`)와 프론트엔드 구조(`CONVENTIONS.md:143-153`)를 그대로
따른다.

```
apps/code-pad/src-tauri/src/
  lib.rs              run(), command 등록, 앱 상태와 watcher manager 보관
  commands/{file,folder,preview,session}.rs
  lsp/                 ← Phase 2; 두 번째 소비자 전까지 앱 로컬
    catalog.rs         관리형·사용자 정의 서버 manifest
    install.rs         다운로드·SHA-256·안전한 archive 설치
    transport.rs       Content-Length stdio JSON-RPC framing
    client.rs          initialize/capability/request/notification
    documents.rs       URI·version·sync·workspace 경계
    positions.rs       LSP position encoding/Windows URI 변환
    process.rs         child lifecycle, stderr, timeout/backoff
  core/               ← 전부 OS 비의존, cargo test 대상
    encoding.rs  line_ending.rs  guard.rs  session.rs
  watcher.rs          notify 기반 manager (IO — core 아님, 앱 상태가 수명 보장)

apps/code-pad/src/
  App.tsx
  editor/             ← 나중에 packages/editor가 될 경계를 미리 그어둔다
    CodeEditor.tsx  extensions.ts  bookmarks.ts
  components/
    ViewPane.tsx  TabBar.tsx  QuickOpen.tsx  PreviewPane.tsx  StatusBar.tsx
```

`watcher.rs`가 `core/` 밖에 있는 이유는 CONVENTIONS §4의 분리 원칙과 정확히 같다 —
`core/`는 OS 의존이 없어야 WSL에서 `cargo test`가 돌고, 파일 감시(`notify`)는 명백한
IO다. everything-plus에는 아직 실제 watcher 구현이 없어(위 "공통 추출" 절 참고)
참고할 기존 패턴이 없다 — code-pad의 `watcher.rs`가 이 저장소에서 `notify`가 실제로
쓰이는 첫 사례다.

watcher의 수명은 `WatcherManager`가 소유한다. `lib.rs`의 앱 상태에 manager를
`Arc`로 보관해 `notify::RecommendedWatcher`가 앱 수명 동안 살아 있게 하고, setup
클로저의 지역 변수로만 두지 않는다. manager는 부모 디렉터리별 watcher와 해당
디렉터리에서 열린 파일명 집합을 관리한다. 파일을 열거나 세션을 복원할 때
`register(path)`, 파일을 닫을 때 `unregister(path)`를 호출하고, 같은 부모 디렉터리를
여러 문서가 쓰면 등록을 합친다. 마지막 문서가 닫힌 부모 디렉터리는 감시를 해제한다.
각 이벤트는 파일명 필터 → debounce → `file-changed` emit 순서를 거친다. watcher
기동 또는 개별 등록에 실패하면 해당 등록만 비활성화하고 mtime+size+content hash 저장 검사를
계속 사용한다.

`editor/`를 `components/`와 분리해 별도 디렉터리로 둔 이유는 주석 그대로다: 두 번째
소비자(knowledge-base)가 생겨 `packages/editor`로 뽑을 때 이동 범위를 이 디렉터리
하나로 한정하려는 사전 경계 긋기다.

## 데이터 흐름

### 함정 ① — CodeMirror는 내부적으로 LF만 쓴다

CRLF 파일을 그대로 CodeMirror에 넣으면 문서 길이와 커서 오프셋이 실제 파일 바이트
기준과 어긋난다. **열 때 LF로 정규화하고, 저장할 때 원래 줄바꿈으로 되돌린다.**
줄바꿈 종류(`line_ending`)는 에디터 버퍼가 아니라 문서 메타데이터로만 들고 다닌다 —
버퍼 자체는 항상 LF라는 불변식을 지켜야 오프셋 계산이 CodeMirror의 가정과 어긋나지
않는다.

### 함정 ② — 저장은 "삭제 후 재생성"일 때가 많다

많은 도구(에디터, `git checkout` 포함)가 저장을 삭제 후 재생성으로 구현한다. 파일
단위로 watch를 걸면 첫 변경 이벤트 이후 그 inode/handle에 대한 감시가 끊긴다.
**부모 디렉터리를 감시하고 파일명으로 필터링**해야 한다. 한 번의 저장이 여러 이벤트
(삭제, 생성, 수정)를 내므로 디바운스도 필요하다 — 여기서 놓친 이벤트가 있어도
치명적이지 않은 이유는 아래 저장 흐름의 snapshot 비교가 2차 방어선이기 때문이다.

### 함정 ③ — BOM은 인코딩과 별도로 보존해야 한다

`encoding_rs::Encoding::for_bom()`으로 먼저 BOM을 검사한다. `EF BB BF`는 UTF-8,
`FF FE`는 UTF-16LE, `FE FF`는 UTF-16BE로 판정하고 BOM 바이트는 버퍼에 넣지 않는다.
`chardetng`은 UTF-16을 자동 감지하지 않으므로 BOM 없는 UTF-16은 자동 판정하지 않고
UTF-8/CP949 감지 실패 경로(UTF-8 lossy + 수동 선택)로 보낸다. BOM이 없는 입력에서
엄격한 UTF-8을 먼저 확인하고, 그다음 `chardetng` 결과가 EUC-KR(CP949)일 때만
지원 목록의 CP949로 채택한다. detector가 그 밖의 레거시 인코딩을 반환하면 역시
감지 실패로 취급한다.

문서 메타데이터는 `encoding_kind`와 `bom: bool`을 분리한다. 따라서 UTF-8과
UTF-8 BOM을 서로 다른 바이트 표현으로 기억하면서도 인코딩 자체는 UTF-8로 다룬다.
저장 시 UTF-8 BOM은 `EF BB BF`를 앞에 붙이고, UTF-16LE/BE는 `text.encode_utf16()`의
각 code unit을 해당 엔디언 바이트로 직접 쓴다. `bom: true`인 UTF-16LE에는 `FF FE`,
UTF-16BE에는 `FE FF`를 각각 앞에 붙이고 `bom: false`면 붙이지 않아 감지 당시의 BOM
상태를 보존한다.
`encoding_rs`의 UTF-16 인코더를 일반 인코더처럼 호출하면 UTF-8 출력 경로가 되므로
그렇게 하지 않는다. CP949는 `encoding_rs::EUC_KR`로 인코딩하고
`had_errors`가 true면 저장을 중단한다. 지원 인코딩·BOM 파일은 디코드→인코드
왕복 시 원본 바이트(줄바꿈 변환을 적용하지 않은 상태)를 보존해야 한다.

### 흐름

- **열기**: `openFile(path, optionalEncoding)` → metadata(크기·mtime) 조회 →
  64MiB 초과면 읽기 전에 거부, 5MiB 초과면 `read_only` 확정 → 바이트 읽기 →
  encoding이 없으면 `encoding::detect()`, 있으면 BOM까지 엄격히 일치하는 명시적 디코딩 →
  `line_ending::detect()` → LF 정규화 → `OpenedFile { text, encoding, line_ending,
  read_only, size, mtimeNanos, contentHash, lossy }` 반환. Rust 내부 snapshot의 mtime은 epoch 나노초
  `i64`로 보관하되 Tauri/JavaScript wire의 `mtimeNanos`는 lossless decimal
  `string`으로 직렬화한다. `contentHash`는 읽은 원본 바이트의 SHA-256이다.
  `size`, `mtimeNanos`, `contentHash`를 프론트가 파일 snapshot으로 함께 보관한다.
  자동 감지 실패의 lossy buffer는 저장할 수 없고, 사용자가 지원 인코딩을 골라 같은
  파일을 엄격하게 다시 연 결과만 `lossy = false`가 된다.
- **저장**: `saveFile(path, text, encoding, lineEnding, expectedMtimeNanos,
  expectedSize, expectedContentHash, sourceLossy)` → lossy source는 즉시 거부 →
  `expectedMtimeNanos` decimal string을 Rust가 epoch nanos `i64`로 엄격하게 파싱한
  뒤 디스크의 현재 mtime·크기·SHA-256 중 하나라도 expected 값과 다르면
  `Err(Conflict)` → LF를 `lineEnding`으로 변환 → `encoding`으로 인코딩 → 권한을
  적용한 sibling temporary file에 write/flush/sync → target snapshot을 다시 읽어
  세 값을 재검사 → 같은 filesystem의 atomic replace를 수행한다. Windows에서는
  target ACL/attributes를 보존하는 `ReplaceFileW`를 사용한다. commit 뒤 metadata를
  조회해 `SavedFile { mtimeNanos, size, contentHash, durabilityWarning }`를 반환한다.
  반환 snapshot의 `mtimeNanos`도 decimal string이며, 저장 성공 시 프론트는 이 새
  snapshot을 다음 저장의 expected 값으로 교체하고, 충돌 시에는 기존 snapshot과
  dirty 내용을 그대로 유지한다. atomic commit 뒤 directory sync나 metadata refresh가
  실패하면 저장 자체를 실패로 오인하지 않고 성공 결과에 `durabilityWarning`을 싣는다.
  이 snapshot 비교는 watcher가 놓친 레이스(함정
  ②)에 대한 2차 방어선이다 — watcher가 죽어 있어도(아래 에러 처리 표 참고) 남의
  변경을 덮어쓰지 않는다.
- **외부 변경**: watcher → 디바운스 → `app.emit("file-changed")` → 프론트가 그
  문서의 dirty 여부로 자동 리로드/배너를 분기한다(결정 4). 자동 리로드하거나
  사용자가 리로드를 선택하면 새 metadata snapshot도 함께 교체한다.
- **Ctrl+P**: filesystem-limited-walk 후속 PR이 머지된 뒤 `crates/filesystem`의
  `collect_limited(root, 50_000)`으로 **1회** 목록을 만든다. 이 상한은 프론트에서
  자른 뒤가 아니라 Rust walk 루프에서 적용하며, 반환된 `truncated`가 true면
  "폴더가 커서 일부만 색인했습니다" 배너로 안내한다(거부하지 않는다 — 근거는
  "확정된 세부 결정" 2번). 타이핑에 따른 필터링은 전부 프론트에서 한다.
  watcher는 열려 있는 문서 전용이고 이 목록을 갱신하는 데 쓰지 않는다 — 폴더 전체를
  감시하는 비용을 지지 않는다.
- **세션**: 파일 열기·닫기·뷰 이동·활성 탭 변경마다 1초 디바운스로 `session.json`을
  저장한다.

### 프리뷰 흐름

`.md` 프리뷰는 `commands/preview.rs`의 `render_preview(path, content)`가 담당한다.
예정된 `crates/markdown`의 `render()`는 frontmatter가 제거된 본문을 받는 API이므로
preview command가 본문 앞의 YAML frontmatter 블록(`---`부터 닫는 `---`까지)을
앱 로컬 helper로 먼저 제거하고, 메타데이터는 표시하지 않는다. 이는 현재
knowledge-base renderer의 계약(`apps/knowledge-base/src-tauri/src/core/markdown.rs:26-35`)
과 command의 호출 순서(`apps/knowledge-base/src-tauri/src/commands/markdown.rs:43-49`)
를 그대로 따른다.

같은 command가 `load_image(src)` 콜백을 주입한다. 콜백은 현재 문서 부모를 기준으로
상대 경로를 결합하고 작업 폴더 밖으로 나가는 경로를 거부한 뒤, 2MB 이하만 읽어
`data:` URI로 만든다. 원격 `http/https`는 crate의 passthrough 규칙을 따르고, 부재·
크기 초과·루트 밖은 기존 `ImageResult` fallback으로 돌려준다. 따라서 프론트는
파일시스템을 직접 읽지 않는다.

`.mmd`는 Markdown renderer를 거치지 않고 파일 전체를 mermaid source로 넘긴다.
`PreviewPane`은 두 경로 모두 mermaid를 `securityLevel: "strict"`로 초기화하고,
문법 오류 시 마지막 성공 SVG를 유지한다. preview command의 응답은 `.md`에서는
살균된 HTML과 mermaid 목록, `.mmd`에서는 mermaid source를 포함한다.

## 상태 저장

`app_local_data_dir()`(CONVENTIONS §3 데이터 위치 규약, `CONVENTIONS.md:107-116`)
아래 **JSON 한 파일**에 세션과 설정(작업 폴더·최근 파일)을 담는다. 저장할 값이
세션 상태와 설정뿐이라 쿼리할 것이 없다.

최상위 구조는 `version`, `workspace_folder`, `docs`, `views`, `active_view`,
`active_doc_by_view`, `recent_files`다. `docs` 항목에는 파일 경로·커서 위치·북마크만
저장하고 편집 버퍼 내용은 저장하지 않는다. `views`에는 앞서 정한 전역 문서 registry의
ID만 저장해 복원 때도 하나의 `docs` 항목을 두 뷰가 공유하지 않도록 한다. 파일이
소실된 doc ID는 양쪽 view 배열에서 함께 제거하고, 나머지 문서는 생성 순서와 뷰
배치를 유지한다.

이 판단은 devbox-manager의 선례를 그대로 따른 것이다.
`apps/devbox-manager/src-tauri/src/commands/manager.rs:36-53`이
`registry_path()`(`data_dir(app)?.join("registry.json")`) →
`read_registry()`(전체 읽어 `serde_json::from_str`, 실패 시
`.unwrap_or_default()`로 빈 벡터) → `write_registry()`(전체를
`serde_json::to_string_pretty`로 다시 씀)로 정확히 "파일 하나, 통째로 읽고 통째로
쓰기" 패턴을 구현하고 있다. 설치된 앱 목록처럼 code-pad의 세션·설정도 부분 갱신이나
조건 검색이 필요 없는 평평한 구조라 같은 패턴이 그대로 들어맞는다.

반대로 SQLite를 쓰는 앱들(activity-timeline, everything-plus, knowledge-base,
life-log)은 전부 실제 쿼리 요구(FTS5 전문 검색, 인덱싱 상태 조회, 집계)가 있다.
code-pad에는 그런 요구가 없으므로 SQLite를 들이지 않는다 — "SQLite는 쿼리할 것이
있을 때 값을 한다"는 판단 그대로다.

식별자는 기존 10개 앱이 예외 없이 쓰는 `com.workbench.<appname>` 패턴을 기계적으로
따른다(`apps/*/src-tauri/tauri.conf.json`의 `identifier` 필드 확인, 예:
`com.workbench.knowledgebase`, `com.workbench.wsldesktop`) → code-pad는
`com.workbench.codepad`가 된다. 이는 새 결정이 아니라 기존 명명 규칙의 기계적 적용이다.

## 에러 처리

| 상황 | 처리 |
|---|---|
| 파일 없음·권한 없음 | `Err` → 배너 (기존 앱들의 `error` state 패턴, 예: `apps/knowledge-base/src/App.tsx`의 `error` state) |
| 큰 파일(5MiB 초과~64MiB 이하) | 읽기 전용 + 하이라이팅 비활성, 상태바에 사유 표시(임계치 근거는 "확정된 세부 결정" 1번) |
| 매우 큰 파일(64MiB 초과) | 내용을 메모리에 읽기 전에 열기를 거부하고 외부 대용량 파일 도구 사용 안내 |
| 인코딩 감지 실패 | UTF-8 lossy로 열고 표시 + 수동 인코딩 선택 제공. lossy 버퍼는 저장을 거부하고 명시적 인코딩으로 다시 열어야 함(선택 목록은 "확정된 세부 결정" 5번) |
| 저장 시 인코딩 불가 문자 | **저장 중단** + UTF-8 전환 제안. 조용한 손실 금지(CP949 파일에 이모지를 넣는 실제 시나리오) |
| 저장 시 mtime/크기/content hash 충돌 | 저장 중단 + 배너. 임시 파일 작성 직후에도 같은 snapshot을 다시 검사하고, dirty 버퍼와 기존 expected snapshot은 유지 |
| watcher 기동·개별 등록 실패 | 해당 감시만 비활성화. mtime+크기 검사가 폴백이라 치명적이지 않음 |
| `session.json` 손상 | 무시하고 빈 세션 시작 |
| 세션 속 파일 소실 | 그 항목만 건너뛰고 나머지 복원 |
| 프리뷰 렌더 실패 | knowledge-base와 동일 — 마지막 성공 유지 + 배지(근거는 knowledge-base 설계 문서의 결정 6, "마지막 성공 SVG 유지") |

## 테스트

code-pad는 이 저장소에서 Rust core 테스트와 프론트 테스트를 **새 에디터 기능과
함께 설계하는 앱**이다. 이미 `knowledge-base`(`src/App.test.tsx:28-69`와
`src-tauri/src/core/markdown.rs:198-203`)와 `wsl-desktop`(프론트
`src/components/PaneCanvas.test.tsx:72-132`와
`src-tauri/src/commands/terminal.rs:290-305`)이 두 계층을
함께 갖고 있으므로, code-pad가 처음이라는 뜻은 아니다. 현재 Rust 쪽
(`apps/`+`crates/` 전체 `#[test]` 85개, 2026-08-12 기준
`grep -rn '#\[test\]' apps crates | wc -l`로 확인)과 프론트 쪽은 이미 각각 성숙해
있고, 프론트 테스트 인프라는 최근에야 저장소 전체에 깔렸다 — 루트
`package.json`의 `devDependencies`에 `vitest`/`@testing-library/react`/`jsdom`이
추가되어 있고, 모든 앱의 `package.json`이 `"test": "vitest run --passWithNoTests"`를
갖고 있으며, `test(workspace): add vitest and cover pure logic + pane remount
(#46)` 커밋이 `apps/wsl-desktop/src/components/PaneCanvas.test.tsx`를 포함해
`.test.ts(x)` 파일 10개를 저장소 전역에 심었다. CI의 `frontend` 잡이 이미
`pnpm test`를 돌리므로(`.github/workflows/ci.yml:34-35`) code-pad가 vitest
스위트를 추가해도 새 CI 배선이 필요 없다.

knowledge-base 마크다운 프리뷰 설계 문서(2026-08-11)와 wsl-desktop 탭 설계
문서(2026-08-12)는 각자 작성 시점에 "이 저장소에는 프론트 테스트 인프라가 전혀
없다"고 적었고, 그 서술은 각각 렌더링을 Rust에 두는 결정과 vitest를 새로 들이지
않는 결정의 근거였다. 지금은 `#46`으로 사실이 달라졌지만, 두 문서는 고치지
않는다 — 작성 시점에는 정확했고 당시 결정의 근거를 그대로 보존해야 왜 그렇게
결정했는지가 남는다. 이 문서가 지금 시점의 사실관계를 다시 기록하는 것으로
충분하다.

- **Rust `cargo test`**: `encoding`(BOM 우선 UTF-8/UTF-16LE·BE 감지, 엄격한
  UTF-8·CP949 감지, **왕복 검증** — BOM을 포함한 지원 바이트가 디코딩→인코딩 후
  동일, CP949 표현 불가 문자 검출),
  `line_ending`(감지 + LF↔CRLF 왕복), `guard`(임계치 경계), `session`(JSON 왕복,
  손상 입력), `commands/file`(canonical path + LF buffer, atomic save,
  mtime/size/SHA-256 conflict와 pre-replace 재검사, lossy 저장 거부, 내부 epoch nanos `i64`와 wire decimal string의 왕복,
  빈 문자열·음수·overflow timestamp 거부)
- **vitest**: 전역 `docs`와 뷰별 ID 배열의 상태 전이(뷰 간 이동, 마지막 탭 닫기,
  활성 탭 이동, 문서 ID 중복 방지), Ctrl+P 필터 매칭
- **컴포넌트 테스트**: 뷰 사이 문서 이동 시 CodeMirror 인스턴스 유지.
  `apps/wsl-desktop/src/components/PaneCanvas.test.tsx`의 패턴을 그대로 쓴다 —
  구체적으로는 자식 컴포넌트를 모킹해 마운트/언마운트를 스파이로 잡고
  (`PaneCanvas.test.tsx:13-36`), 뷰 전환 전후로 `unmountSpy`가 호출되지 않았는지
  확인한다(`PaneCanvas.test.tsx:84-98`, "탭을 전환해도 비활성화된 팬은 언마운트되지
  않고, 새로 마운트되지도 않는다"). code-pad에서는 "팬"이 "문서 편집기 인스턴스"로,
  "탭 전환"이 "뷰 간 이동"으로 바뀔 뿐 검증 구조는 동일하다.
- **Windows CI**: `ci(workspace): add Windows compile-check job (#45)`로 추가된
  `rust-windows` 잡(`.github/workflows/ci.yml:76-106`)이 `cargo test --workspace`를
  Windows 러너에서 실제로 돌린다(`ci.yml:104-106`). CP949 인코딩 처리처럼 플랫폼에
  실제로 의존하는 동작을 이 잡이 실기에서 컴파일·테스트한다 — ubuntu 전용 `rust`
  잡만으로는 Windows 전용 코드 경로가 컴파일조차 안 될 수 있다는 점을
  `ci.yml:82-83`의 주석("`#[cfg(target_os = "windows")]` 코드는 여기서만 실제로
  컴파일된다")이 이미 명시하고 있다.
- **수동 검증(Windows)**: 실제 CP949 파일, CRLF 파일, 큰 파일, git 브랜치 전환
  중 리로드.

## 범위 밖

- 매크로, PDF, 바이너리/hex 편집, 터미널 내장, git 통합, 파일 트리 사이드바 —
  Phase 1 제외 목록. 파일 트리는 결정 1의 근거(다른 두 앱이 이미 담당)로 제외.
- LSP 구현 자체는 Phase 2 이후의 코드 PR 범위다. 이 문서에는 Phase 2의 인터페이스와
  운영·보안 계약을 정하지만, 서버 바이너리와 앱 코드는 추가하지 않는다. 공통
  `crates/lsp`는 두 번째 실제 소비자가 생긴 뒤에만 추출한다.
- `packages/editor` 추출 — code-pad가 CodeMirror의 첫 소비자인 동안은 대상이
  아니다("공통 추출" 절).
- 미저장 버퍼 내용의 세션 복원 — 결정 3의 근거로 제외.
- **여러 파일에 걸친 검색(프로젝트 전체 grep)** — everything-plus가 이미 이
  역할을 담당한다(배경 절, "everything-plus는 이름 그대로 로컬 파일 검색이
  본업"). code-pad가 담당하는 것은 단일 파일 내 찾기/바꾸기뿐이다("확정된 세부
  결정" 6번). 같은 기능을 두 앱에 중복으로 넣는 것은 결정 1에서 파일 트리를
  두지 않은 것과 같은 이유로 피한다.

## 확정된 세부 결정

이 브리핑에는 구체적인 값 없이 남아 있던 항목들이다. 아래 8건을 모두 확정한다.

### 1. 큰 파일 임계치 → 편집 5MiB, 열기 64MiB

**결정**: 5MiB를 넘고 64MiB 이하면 읽기 전용 + 하이라이팅 비활성으로 연다.
64MiB를 넘으면 전체 버퍼를 할당하기 전에 열기를 거부한다.

**근거**: CodeMirror 6은 뷰포트 단위로만 DOM을 그리므로 문서 크기 자체는 큰
파일도 버티지만, 문법 하이라이팅을 담당하는 Lezer 증분 파서는 문서가 커질수록
파싱 비용이 늘어나 수 MB급 파일에서 입력 지연으로 체감된다. 이 상한을
everything-plus의 `MAX_CONTENT_BYTES`(1MB, `apps/everything-plus/src-tauri/src/core/indexer.rs:13`)나
knowledge-base의 이미지 인라인 2MB 상한(`docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:112`)과
다르게 잡는 이유: 그 두 값은 "인덱싱 대상 파일 전체" 또는 "문서 하나에 박힌
이미지 여러 개"에 반복 적용되는 **누적** 비용 기준이고, code-pad의 5MiB는
"사용자가 지금 연 파일 하나"에 대한 **단발** 비용 기준이다 — 적용되는 횟수
자체가 다르므로 같은 값을 재사용할 근거가 없다.

64MiB 절대 상한은 편집 성능 기준이 아니라 메모리 안전 경계다. 파일 크기를 먼저
조회하고 상한을 넘으면 `fs::read`를 호출하지 않으므로 sparse/로그/덤프 파일을 잘못
선택해 프로세스 메모리를 고갈시키는 일을 막는다.

### 2. Ctrl+P 목록 상한 → 50,000개, 초과 시 잘라내고 배너 안내

**결정**: filesystem-limited-walk 후속 PR의 `collect_limited`에
`max_entries = 50_000`을 넘겨 순회 중 그 수에 도달하면 즉시 중단한다. 프론트에는
그때까지의 목록과 `truncated` 플래그만 전달하고, 그 이상을 거부하지 않는다.
잘렸으면 "폴더가 커서 일부만 색인했습니다" 배너로 안내한다.

**근거**: PR #48로 현재 `crates/filesystem`에 들어온 ignore 규칙이
`node_modules`/`target`/`.git` 등 대용량 디렉터리를 이미 걸러낸다
(`crates/filesystem/src/ignore.rs:1-24`의 18개 이름 매치). 일반적인 프로젝트 폴더는
이 필터를 거치면
5만 개 근처에도 가지 않는다. 드물게 닿는 경우(모노레포 루트를 통째로 여는 등)에도
Ctrl+P를 완전히 못 쓰게 거부하는 것보다 앞부분만으로라도 동작하는 편이 낫다.
앞부분의 순서는 파일시스템 walk 순서이며 최근 수정 파일 우선이라는 보장은 하지
않는다. 빠른 열기에서 최근 파일을 우선하려면 별도 정렬·순위화 기능으로 명시해야
하므로 Phase 1에서는 잘림과 그 한계를 배너로만 알린다.

### 3. 자동완성 소스 → 현재 문서만

**결정**: `@codemirror/autocomplete`의 기본 동작(현재 문서의 단어 목록) 그대로
쓴다. 다른 뷰의 문서나 Ctrl+P가 만든 폴더 인덱스로 범위를 넓히지 않는다.

**근거**: 두 가지다. (1) 범위를 넓히면 지금 편집 중인 파일과 무관한 제안이
섞여 자동완성의 신호 대 잡음비가 나빠진다. (2) 진짜 심볼 수준 완성(정의로
이동, 타입 인지, import 자동 완성)은 **Phase 2의 LSP가 할 일**이고, 단어 기반
자동완성은 그 전까지의 최소 기능이다. 지금 범위를 넓혀 두면 LSP가 들어올 때
두 완성 소스가 겹쳐 어느 쪽 제안인지 구분하기 어려워진다.

### 4. 북마크 → 세션에 저장

**결정**: `session.json`에 파일별 줄 번호 배열로 저장한다.

**근거**: 세션 복원이 이미 커서 위치까지 복원하기로 확정돼 있다("결정과 근거"
절 3번). 같은 "문서 안 위치 정보"인 북마크만 휘발시키면 두 기능이 서로 다른
영속성 규칙을 갖게 되어 사용자 입장에서 일관성이 없다.

**한계(수용함)**: 파일이 외부에서 변경되면(git checkout, 다른 에디터로 편집
등으로 줄이 삽입·삭제되면) 저장된 줄 번호가 실제 의도한 위치와 어긋난다.
Notepad++도 같은 한계를 갖고 있다 — 북마크를 diff 기반으로 재계산해 따라가게
하려면 별도 알고리즘이 필요한데, Phase 1에서 들일 비용이 아니라고 판단해
그대로 수용한다.

### 5. 수동 인코딩 선택 목록 → 감지 대상과 동일한 5종

**결정**: UTF-8, UTF-8 BOM, UTF-16LE, UTF-16BE, CP949(EUC-KR) 5종만 수동 선택
드롭다운에 노출한다.

**근거**: `encoding_rs`는 이보다 훨씬 많은 인코딩을 지원하지만, 목록을 넓히면
UI만 지저분해질 뿐 이 앱의 실사용 시나리오(한국어 로컬 개발 환경에서 마주치는
CP949 레거시 파일)에서 나머지 인코딩을 고를 일이 없다. 감지 대상과 선택 목록을
같은 5종으로 맞추면 "감지가 왜 이 인코딩은 후보에 안 넣어줬지"라는 사용자
혼란도 없앤다. UTF-8 BOM은 별도 인코딩이 아니라 `bom: true`인 UTF-8 표현이며,
UTF-16LE/BE도 BOM이 있으면 그 상태를 문서 메타데이터에 보존한다. BOM 없는
UTF-16은 자동 감지하지 않는다는 한계는 상태바와 수동 선택 안내에 표시한다.

### 6. 찾기/바꾸기 → Phase 1에 포함(설계 누락 정정), 다중 파일 검색은 범위 밖

**결정**: Phase 1 기능 목록에 "단일 파일 내 찾기/바꾸기(정규식 포함)"를
추가한다. `@codemirror/search`를 직접 의존성으로 선언해 쓴다. 여러 파일에 걸친
검색(프로젝트 전체 grep)은 포함하지 않는다.

**근거**: 원 요구사항의 Phase 1 기능 목록과 제외 목록 어디에도 찾기/바꾸기가
없었다 — Notepad++를 대체하는 것이 목적인 에디터에 찾기/바꾸기가 없으면 애초에
Notepad++ 대신 쓸 수 없으므로 이는 누락이었다. `@codemirror/search`는 검색
패널·정규식·전체 바꾸기를 이미 다 구현해 제공한다. `codemirror`의 의존성 그래프에
들어 있더라도 pnpm workspace의 앱이 그 패키지를 직접 import하려면
`apps/code-pad/package.json`에 직접 선언하고 lockfile에 기록해야 한다. 여러 파일
검색을 포함하지 않는 이유는 결정 1(파일 트리를 두지 않는 이유)과 같다 — everything-plus가
이미 로컬 파일 검색을 본업으로 하고 있어("배경" 절), code-pad가 같은 기능을
다시 만들면 세 번째 파일 트리를 만드는 것과 같은 종류의 중복이 된다.

### 7. `session.json`에 `version` 필드를 처음부터 둔다

**결정**: `session.json` 최상위에 `version` 필드를 둔다. 읽을 때 코드가
기대하는 값과 다르면 손상된 파일과 동일하게 취급해 무시하고 빈 세션으로
시작한다(위 에러 처리 표의 "`session.json` 손상" 행과 같은 경로를 그대로
재사용한다 — 새 분기를 만들지 않는다).

**근거**: everything-plus가 정확히 이 문제를 뒤늦게 겪었다. 초기 스키마
(`core/db.rs`의 `migrate()`)는 버전 개념 없이 시작했다가, 경로 정규화
(`normalize_path()`) 도입 과정에서 "이전 방식으로 저장된 DB와 새 방식으로
저장된 DB를 어떻게 구분할지"라는 문제에 부딪혀 뒤늦게 `meta(schema_version)`
테이블을 추가해야 했다 — 테이블 정의는
`apps/everything-plus/src-tauri/src/core/db.rs:95-98`, 버전을 비교해 낮으면
파생 데이터를 지우고 버전을 갱신하는 로직은 `db.rs:108-127`
(`fix(everything-plus): repair indexing lifecycle (#42)`, 커밋 메시지 그대로
"Added a meta(schema_version) row; migrate() clears the derived index
(clear_all) whenever the stored version is behind, then bumps it"). 처음부터
버전 필드가 있었다면 "버전이 낮으면 지운다"는 한 줄로 끝났을 문제가, 없었기
때문에 별도 PR과 마이그레이션 분기를 요구했다.

code-pad의 경우는 오히려 더 단순하게 처리할 수 있다 — everything-plus는
`roots`(사용자가 등록한 실제 설정값)는 보존하고 파생 인덱스(`files`/
`file_content`)만 지웠지만, code-pad의 `session.json`은 열린 파일 목록·뷰
배치·커서 위치·북마크 전부가 다시 열면 재구성되는 파생 상태다(결정 3에서
미저장 버퍼만 예외로 뒀을 뿐, 그 외엔 전부 파생 가능하다는 것이 이미 전제였다).
그래서 버전 불일치 시 부분 보존 없이 통째로 버려도 무손실이다.

### 8. expected file snapshot → epoch nanos + 크기 + SHA-256 병행 비교

**결정**: `SystemTime`을 epoch 나노초 기준 `i64`로 변환해 Rust 내부 snapshot의
mtime으로 보관한다. 단, epoch nanos는 JavaScript `number`의 safe integer 범위를
넘으므로 Tauri/JavaScript wire의 `mtimeNanos`와 `expectedMtimeNanos`는 반드시
십진수 `string`으로 직렬화한다. 열 때 읽은 파일 크기는 `expectedSize: u64`, 원본
바이트 SHA-256은 `expectedContentHash: string`으로 함께 보관한다. 저장 커맨드는
`saveFile(path, text, encoding, lineEnding, expectedMtimeNanos, expectedSize,
expectedContentHash, sourceLossy)` 형태로 세 snapshot 값과 lossy provenance를 받아 Rust에서
mtime string을 엄격하게 non-negative decimal `i64`로 파싱한다. 빈 문자열·음수·
숫자가 아닌 값·`i64` 범위 초과는 metadata 비교나 쓰기 전에 거부한다. 디스크의
현재 mtime·크기·content hash가 하나라도 다르면 충돌(`Err(Conflict)`)로 처리한다.
temporary file을 완성한 직후에도 target의 세 값을 다시 검사한다. 저장에
성공하면 커맨드가 실제로 다시 조회한 `{ mtimeNanos: string, size, contentHash,
durabilityWarning }`를 반환하고,
프론트는 이를 다음 저장의 새 expected snapshot으로 교체한다. 외부 변경을
리로드한 경우에도 같은 방식으로 snapshot을 갱신한다.

**근거**: 밀리초 단위로는 같은 밀리초 안에서 연속으로 파일이 쓰였을 때(예:
빌드 스크립트가 파일을 빠르게 여러 번 저장하는 경우) 서로 다른 두 저장을
같은 값으로 오인할 수 있다. 나노초 단위 `i64`는 1970년부터 약 292년(2262년경)
까지 표현 가능해 이 앱의 수명 안에서 오버플로를 걱정할 필요가 없다.

파일시스템별 mtime 정밀도 차이(NTFS는 100ns 단위, FAT류는 2초 단위) 때문에 같은
mtime과 크기를 유지한 채 내용만 바뀔 수 있다. SHA-256을 함께 비교해 이 경우도
막는다. 저장 가능한 파일은 최대 5MiB라 두 번의 hash 비용은 경계가 명확하다.
snapshot 갱신을 저장 성공의 일부로 명시하는 이유는
첫 저장 후에도 이전 open 시점의 expected 값을 계속 쓰면 앱 자신의 저장을 다음
저장의 외부 충돌로 오인하기 때문이다.

## 구현 순서

각 단계가 독립적으로 검증 가능하도록 순서를 잡는다. 완료 정의는 저장소 전체와
동일하게 `cargo test` + `cargo check` + `pnpm build`다(`AGENTS.md:29`).

### 전제 — 반드시 code-pad 구현보다 먼저, 별도 PR로

기존 앱(everything-plus, knowledge-base)을 건드리는 리팩터이기 때문에 code-pad
구현과 한 PR에 섞으면 문제가 생겼을 때 원인이 "추출 자체의 실수"인지 "code-pad가
새 인터페이스를 잘못 썼는지" 분리할 수 없다. 현재 `crates/filesystem`의 무제한
API는 이미 #48로 머지됐지만, 아래 두 prerequisite PR을 code-pad 구현보다 먼저
merge한다.

0a. **filesystem 제한 순회 후속 PR** — 현재 `crates/filesystem`의
   `collect(root) -> Vec<IndexedFile>`는 그대로 두고, 브랜치
   `feat/crates/filesystem-limited-walk`에서 `collect_limited(root, max_entries)`와
   `LimitedCollect { files: Vec<IndexedFile>, truncated: bool }` 반환 타입을
   추가한다. Rust walk 루프에서 상한을 적용하고, 기존 `collect`를 호출하는 everything-plus의 무제한
   인덱싱 동작과 `apps/everything-plus/src-tauri/Cargo.toml:27` path 의존은 바꾸지
   않는다. 검증: 현재 crate의 `crates/filesystem/src/walk.rs:58-71` 순회 테스트와
   새 제한·`truncated` 테스트, everything-plus의
   `apps/everything-plus/src-tauri/src/core/indexer.rs:19-27` 텍스트 확장자 테스트,
   `crates/filesystem/src/ignore.rs:30-43` 제외 규칙 테스트가 통과해야 한다 +
   `cargo check --workspace`.
0b. **`crates/markdown` 추출 PR** — 아직 존재하지 않는 future prerequisite다.
   code-pad 구현 전에 knowledge-base의 `core/markdown.rs`를 `crates/markdown/`으로
   이동하고 knowledge-base를 path 의존으로 재연결한다. generic crate에 없는
   `crate::core::frontmatter` 테스트 import는 body-only fixture로 바꾼다. 검증: 기존 9개 테스트
   (`core/markdown.rs`의 `#[cfg(test)] mod tests`, 이 문서에서 읽어 확인한
   `frontmatter_is_stripped_before_render`부터 `non_mermaid_code_block_is_untouched`까지)가
   이동 후에도 그대로 통과해야 한다 + `cargo check --workspace`.

### code-pad 구현

1. 앱 스캐폴드: CONVENTIONS §6 절차(`CONVENTIONS.md:181-187`)로
   `pnpm create tauri-app` → 4곳 이름 교체 → 루트 `Cargo.toml`의 `members`에
   `apps/code-pad/src-tauri` 추가 → `tauri.conf.json` identifier
   `com.workbench.codepad`. 이어서 앱의 `package.json`에 직접 import하는 모든
   CodeMirror 패키지(`codemirror`, `@codemirror/state`, `@codemirror/view`,
   `@codemirror/commands`, `@codemirror/language`, `@codemirror/search`,
   `@codemirror/autocomplete`, 선택한 `@codemirror/lang-*`)를 선언하고
   `pnpm install`로 lockfile을 갱신한다. pnpm에서는 transitive dependency를
   직접 import할 수 없으므로 `@codemirror/search`도 반드시 이 목록에 포함한다.
2. `core/encoding.rs`: BOM 우선 감지, 엄격한 UTF-8/CP949 제한, UTF-16 수동
   엔디언 인코딩 + BOM 보존, 왕복 검증 테스트 →
   `cargo test`.
3. `core/line_ending.rs`: 감지 + LF↔CRLF 왕복 테스트 → `cargo test`.
4. `core/guard.rs`: 편집 5MiB·열기 64MiB 임계치 경계 테스트("확정된 세부 결정" 1번) → `cargo test`.
5. `core/session.rs`: JSON 왕복 + 손상 입력 테스트 → `cargo test`.
6. `commands/file.rs`: open/save 커맨드, 내부 `i64` snapshot과 wire
   `mtimeNanos`/`expectedMtimeNanos` decimal string 변환,
   `expectedSize`/`expectedContentHash` 비교와 lossy 저장 거부,
   성공 후 새 snapshot 반환 → `cargo check`.
7. `commands/preview.rs` + `watcher.rs`: `.md` frontmatter 제거·이미지 loader·
   `crates/markdown` 연결, 그리고 앱 상태가 소유하는 watcher manager의 부모
   디렉터리 감시·파일명 필터링·동적 register/unregister·디바운스(함정 ② 반영) →
   `cargo check`.
8. 프론트 `editor/`: CodeMirror 6 배선(`CodeEditor.tsx`, `extensions.ts`),
   `@codemirror/lang-*` 언어팩으로 문법 하이라이팅 → `pnpm build`.
9. `@codemirror/search` 패널 배선(찾기/바꾸기, 정규식) — 직접 선언한 패키지를
   사용해 단일 문서 편집기가 이미 준비된 8단계 직후에 붙인다("확정된 세부 결정"
   6번) → `pnpm build`.
10. 프론트 전역 문서 registry + 뷰별 ID 상태(`docs`/`views`/`activeView`) +
    `ViewPane.tsx`: `PaneCanvas.tsx`의 "하나의 안정된 부모 + CSS `display`/`order`"
    패턴을 그대로 적용(아래 "뷰 사이 문서 이동" 참고) → vitest(뷰 간 이동/마지막
    탭 닫기/활성 탭 이동/ID 중복 방지) +
    컴포넌트 테스트(`PaneCanvas.test.tsx` 패턴 재사용).
11. `QuickOpen.tsx`: filesystem-limited-walk 후속 PR의 Rust-side
    `collect_limited(..., 50_000)` 목록과 `truncated` banner + 프론트 필터링 →
    vitest 필터 매칭.
12. `PreviewPane.tsx`: `commands/preview.rs` 응답과 `crates/markdown` 연결(`.md`),
    strict mermaid `.mmd` 단독 파일 렌더 분기 → `pnpm build`.
13. `StatusBar.tsx`: 인코딩/줄바꿈/읽기전용 표시 배선.
14. 멀티커서/사각선택/북마크(`editor/bookmarks.ts`, 세션 저장 — "확정된 세부
    결정" 4번)/단어 기반 자동완성(`@codemirror/autocomplete`, 현재 문서 한정 —
    "확정된 세부 결정" 3번) CodeMirror 확장 배선 → `pnpm build`.
15. Windows에서 `pnpm tauri dev`로 수동 검증: 실제 CP949 파일, CRLF 파일, 큰
    파일, git 브랜치 전환 중 리로드(WSL에서는 `pnpm tauri dev`를 돌릴 수 없으므로
    이 단계만 Windows 필요, `CONVENTIONS.md §1`).

### 뷰 사이 문서 이동 — wsl-desktop 교훈 적용 (10단계 상세)

`apps/wsl-desktop/src/components/PaneCanvas.tsx`에서 실제로 터진 문제다. 자식을
다른 부모의 children 배열로 옮기면 React가 언마운트 후 재마운트한다(key는 같은
부모 안에서만 유효하다). CodeMirror 인스턴스가 날아가면 **실행취소 히스토리와
스크롤 위치가 사라진다** — xterm 스크롤백보다 체감이 크다(코드 에디터에서 undo
히스토리 손실은 작업 내용 손실로 바로 이어진다).

wsl-desktop은 처음에 React Portal로 풀려다 실측으로 폐기했다 — portal은 DOM
출력 위치만 바꿀 뿐 React 엘리먼트 트리상의 부모(그 `createPortal` 호출이 어느
JSX의 자식으로 등장하는가)는 그대로이므로, 활성 탭용 `.map()`과 비활성 탭용
`.map()`을 오가면 결국 "다른 부모의 children 배열 사이 이동"이 되어 막으려던
버그가 그대로 남았다(`docs/superpowers/specs/2026-08-12-wsl-desktop-tabs-design.md:149-169`).

채택된 해법은 **전역 `docs` registry의 모든 문서를 하나의 editor host 부모 아래
안정된 배열 순서로 두고, 뷰 소속·표시 여부·순서는 CSS와 ID 배열로만 제어**하는
것이다. `ViewPane.tsx`는 각 뷰의 탭 바와 레이아웃을 담당하지만
`CodeEditor`를 별도의 `views[0].map()`/`views[1].map()` 부모로 나누지 않는다.
`docs` 배열 자체는 절대 재정렬하지 않는다. 실제 구현 요지는 다음과 같다:

```tsx
// PaneCanvas.tsx:58-88 요지 — code-pad의 DocHost가 이 구조를 그대로 쓴다
<div className="doc-host" style={gridStyle}>
  {docs.map((doc) => {
    const placement = placementForDoc(doc.id, views);
    const visible = placement !== null;
    return (
      <CodeEditor
        key={doc.id}
        /* ... */
        style={visible ? placement.style : { display: "none" }}
      />
    );
  })}
</div>
```

`docs` 배열은 추가는 `[...prev, doc]`(append만), 제거는 `filter`만 써서 상대 순서를
보존한다. `views[0]`과 `views[1]`에는 doc ID가 중복 없이 한 번씩만 들어가며,
문서를 옮길 때는 이 두 배열의 ID 소속만 바꾼다. `.doc-host`의 자식 목록이 항상
"지금까지 만들어진 모든 문서, 생성 순서 그대로"이고 뷰 전환·문서 이동으로 바뀌는
것이 각 자식의 placement/style뿐이라면, React는 매번 같은 부모의 같은 위치에서
같은 key를 발견하므로 재조정만 하고 fiber를 옮길 일 자체가 없다.

`PaneCanvas.test.tsx`가 이 불변을 검증하는 방식도 그대로 재사용한다 — 자식 컴포넌트를
모킹해 마운트/언마운트를 스파이로 잡고(`PaneCanvas.test.tsx:13-36`), 전환 전후로
`unmountSpy`가 호출되지 않았음을 확인한다(`PaneCanvas.test.tsx:84-98`,
"탭을 전환해도 비활성화된 팬은 언마운트되지 않고, 새로 마운트되지도 않는다").
code-pad에서는 CodeMirror 인스턴스를 감싸는 컴포넌트를 모킹해 같은 스파이 패턴을
적용하고, `docId`를 `views[0]`에서 `views[1]`로 옮긴 뒤 `docs` 배열의 순서가
그대로인지도 확인한다. 이 전역 registry 불변식 없이는 결정 2의 `views` 모델이
안정된 부모 패턴을 우회하게 되므로 구현 전에 이 테스트를 먼저 고정한다.

## Phase 2 — LSP

### 목표와 불변식

Phase 2는 CodeMirror에 특정 언어의 분석기를 심는 작업이 아니라, LSP 3.17을 말하는
언어 중립 클라이언트와 로컬 서버 수명 관리자를 추가하는 단계다. 클라이언트는 서버가
무엇을 분석하는지 알 필요 없이 URI, 문서 버전, capability, JSON-RPC 요청을 전달한다.
언어별 차이는 카탈로그의 `language_id`, 파일 확장자, 실행 명령, 런타임, 초기화 옵션으로
한정한다. 표준 LSP가 아닌 서버별 확장은 카탈로그 어댑터에 격리하고, Phase 2의
공통 API에는 넣지 않는다.

초기 사용자 기능은 diagnostics, completion, hover, definition, references, rename,
formatting이다. 단어 기반 CodeMirror 자동완성은 서버가 없거나 completion capability를
광고하지 않을 때도 계속 동작한다. 다른 기능도 서버가 없으면 버튼을 숨기거나
"언어 서버를 설치·활성화하세요"라는 상태만 표시하며, 파일 열기·편집·저장·프리뷰·찾기/
바꾸기를 막지 않는다. LSP는 편집기의 보조 기능이지 파일 편집 경로의 선행 조건이
아니다.

실행 중인 서버의 네트워크 endpoint(TCP/remote LSP)는 지원하지 않는다. 관리형
artifact를 HTTPS로 내려받는 설치 경로는 아래 보안 절차로 명시적으로 허용하지만,
실행 대상은 (a) code-pad가 명시적으로
설치한 로컬 서버, (b) 사용자가 이미 설치한 로컬 실행 파일, (c) 사용자가 직접 등록한
stdio 실행 파일뿐이다. 서버 설치·업데이트는 항상 사용자가 누른 확인 동작으로
시작하며, 앱 시작·파일 열기·백그라운드 타이머가 자동으로 다운로드하거나 "latest"를
따라가지 않는다. 설치 화면에는 서버 이름, 고정 버전, source, license, artifact,
SHA-256, 필요한 런타임, 설치 크기를 표시하고 사용자가 확인한 뒤에만 진행한다.

### 현재 검증된 초기 카탈로그

아래는 2026-08-12에 package metadata와 각 프로젝트의 공식 release/source를 조회해
확인한 **설계 기준 스냅샷**이다. URL은 조회 결과에 존재하는 주소만 적는다. 구현 시
카탈로그 항목을 갱신할 때도 먼저 같은 메타데이터와 release asset을 조회하고, URL을
버전·플랫폼별 manifest에 그대로 복사한 뒤 digest를 별도로 검증한다. URL 패턴을
문자열로 조합해 추측하지 않는다. SHA-256은 다운로드한 바이트의 `sha256sum`과
registry/GitHub가 제공한 artifact를 대조한 값이다.

| 언어·파일 | source / license | 고정 artifact와 SHA-256 | 실행·런타임 확인 |
|---|---|---|---|
| Rust (`.rs`) | [rust-analyzer](https://github.com/rust-lang/rust-analyzer) / MIT OR Apache-2.0 | [2026-08-10.1 Windows x64 zip](https://github.com/rust-lang/rust-analyzer/releases/download/2026-08-10.1/rust-analyzer-x86_64-pc-windows-msvc.zip) / `f667620d3af202f480faf9e407374509ebddef3b8611922e463aeaa7e6985fc8` | archive 안 `rust-analyzer.exe`; native Windows 실행 파일, 별도 Node 불필요 |
| TypeScript·JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`) | [typescript-language-server](https://github.com/typescript-language-server/typescript-language-server) / Apache-2.0 | [npm `typescript-language-server@5.3.0`](https://registry.npmjs.org/typescript-language-server/-/typescript-language-server-5.3.0.tgz) / `398cacc17fff2108652e7b4050e3182008d17063246b3fea7dcf5fae2ce1560e`; paired [TypeScript `6.0.3`](https://registry.npmjs.org/typescript/-/typescript-6.0.3.tgz) / `33cd0ee1beaa8c9e9d15a9da836c62ddea4c34a42d7c2d349dbc80d94165d22a` | `typescript-language-server --stdio`; server package metadata의 Node `>=20`, paired TypeScript metadata의 Node `>=14.17`. upstream README가 두 package를 함께 설치하므로 두 artifact 모두 reviewed lock에 고정 |
| Python (`.py`, `.pyi`) | [basedpyright](https://github.com/DetachHead/basedpyright) / MIT | [npm `basedpyright@1.39.9`](https://registry.npmjs.org/basedpyright/-/basedpyright-1.39.9.tgz) / `5e92f462d04d91fe1370d65cbb1ac241c0c62b3f2c893c4e0b1bf9a82c9e99b2` | `basedpyright-langserver --stdio`; package metadata의 Node `>=14`. `pyright-langserver` alias도 제공되지만 catalog은 `basedpyright-langserver`를 사용 |
| JSON·HTML·CSS (`.json`, `.jsonc`, `.html`, `.htm`, `.css`, `.scss`, `.less`) | [vscode-langservers-extracted](https://github.com/hrsh7th/vscode-langservers-extracted) / MIT | [npm `vscode-langservers-extracted@4.10.0`](https://registry.npmjs.org/vscode-langservers-extracted/-/vscode-langservers-extracted-4.10.0.tgz) / `d6e2d090d09c4b91daa74e9e7462a3d3f244efb96aa5111004cfffa49d6dc9ef` | `vscode-json-language-server`, `vscode-html-language-server`, `vscode-css-language-server`; package `bin` entry가 `createConnection()`의 stdio 기본값으로 시작하며 dependency 목록을 기준으로 reviewed Node lock 필요 |

`typescript-language-server`와 `vscode-langservers-extracted`의 npm tarball은 실행 파일과
모든 dependency를 하나의 self-contained binary로 만들지 않는다. 전자는 README에서
TypeScript를 함께 설치하도록 하고, 후자는 package metadata에 `vscode-*-languageservice`,
`vscode-languageserver`, `typescript` 등의 dependency를 명시한다. 따라서 installer가
root tarball 하나만 풀어 "설치 완료"로 표시하면 안 된다. 이 두 항목은 카탈로그에
reviewed npm lock manifest(모든 transitive package의 exact version, registry URL,
SHA-256)를 함께 넣고, 그 lock에 기록된 tarball만 내려받는다. lock이 없거나 digest가
없는 dependency가 있으면 설치를 거부한다. `basedpyright` npm package는 `bin`에
`basedpyright-langserver`와 `pyright-langserver`를 내보내며, 공식 문서도 stdio 인자를
사용한 IDE 설정을 보여 주므로 위 명령을 사용한다. Node 자체는 앱에 번들하지 않는다;
시스템 Node 또는 사용자가 지정한 Node 경로가 없으면 해당 서버만 unavailable로
표시한다.

이 스냅샷에서 `typescript-language-server@5.3.0`은 workspace에
`typescript@6.0.3`을 둔 임시 프로젝트로 `initialize` smoke를 통과했고 서버가
workspace TypeScript 경로를 선택한다는 log를 남겼다. 반대로 registry의 더 최신
TypeScript `7.0.2`는 같은 server의 "valid TypeScript installation" 검사에서
거부됐으므로 기본 paired runtime으로 선택하지 않는다. 향후 server가 바뀌면 이
호환성 smoke를 다시 통과한 exact TypeScript version만 lock에 넣는다. 서버가
workspace의 TypeScript를 선택할 때도 허용 범위 검사를 하고, 임의의 global
TypeScript를 조용히 사용하지 않는다.

향후 추가 가능한 항목은 같은 schema의 catalog entry로만 확장한다. 예시는 Go의
`gopls`, C/C++의 `clangd`, Java의 `jdtls`, C# 서버, Lua 서버, YAML 서버다. 각 entry는
그 시점의 공식 release/package metadata와 Windows 실행 방법을 확인한 뒤 version,
license, source, artifact, digest, runtime, command를 채워야 하며, 현재 Phase 2에서
미리 URL이나 버전을 결정하지 않는다.

### 서버 manifest와 설정 경계

카탈로그 entry는 다음 필드를 갖는 Rust 구조체(저장 시 JSON)로 정의한다. `id`와
`version`은 설치 경로와 상태의 key이며, `platform`은 `windows-x86_64`처럼 명시한다.

```text
ServerManifest {
  id: string,                         // rust-analyzer, typescript-language-server, ...
  version: string,                    // exact, never a range or latest
  platform: string,
  languages: [{ language_id, extensions }],
  source_url: https URL,              // upstream source/repository
  license: SPDX string,
  artifact: { kind, url, sha256, size_bytes, archive_root },
  runtime: { kind: native | node, executable, min_version? },
  command: { executable, args: [string] },
  files: { entrypoint: relative path, package_lock_sha256? },
  capabilities_hint: optional,        // UI hint only; server response wins
  generated_at: RFC3339,
}
```

`capabilities_hint`는 미리 UI를 그리기 위한 정보일 뿐 실제 사용 가능 여부를 결정하지
않는다. `initialize` 응답이 권위 있는 capability source다. manifest의 `source_url`,
`license`, `version`, `sha256`는 설치 metadata와 UI에 그대로 보존한다. license가
불명확하거나 source와 artifact의 관계를 확인할 수 없는 항목은 카탈로그에 추가하지
않는다.

서버 종류는 세 가지다.

1. **managed** — catalog manifest와 reviewed dependency lock으로 설치한 서버. 앱은
   실행 파일과 package tree를 `%LOCALAPPDATA%\\com.workbench.codepad\\lsp\\servers\\`
   아래 versioned directory에 저장한다. 실제 파일은 번들에 들어가지 않는다.
2. **local** — 사용자가 이미 설치한 executable/runtime 경로를 선택한 서버. 앱은
   `canonicalize`와 실행 권한을 확인하지만 파일을 복사하거나 업데이트하지 않는다.
3. **custom** — 사용자가 `executable`과 고정 `args`를 등록한 stdio 서버. shell 문자열,
   pipe 문법, TCP 주소는 받지 않고 `Command`의 argv로만 실행한다. custom 서버도
   source/license/version을 사용자가 입력하게 하며, 모르는 값은 "사용자 제공/확인 안 됨"
   으로 표시한다. 임의의 remote download를 custom entry로 우회할 수 없다.

`apps/code-pad/src-tauri/src/lsp/`가 Phase 2의 구현 경계다. 현재 저장소에는 두 번째
실제 LSP 소비자가 없고, CONVENTIONS의 "두 번째 앱에서 실제로 필요해진 코드만
`crates/`로 추출" 규칙과 기존 문구 `crates/lsp`가 충돌한다. 이 설계에서는 규칙을
우선해 `crates/lsp`를 만들지 않는다. 향후 두 번째 앱이 같은 client/transport를
실제로 import하는 별도 PR이 생기면, 이 디렉터리에서 protocol-independent 모듈과
테스트를 `crates/lsp`로 옮기고 두 앱을 path dependency로 연결한다. 그때까지
code-pad 전용 workspace/session 정책은 앱에 남긴다.

### 런타임 아키텍처

```text
CodeMirror document
       │ editor transaction / cursor
       ▼
frontend LspAdapter ── Tauri invoke/event ──► LspManager (Rust)
                                                │ one Session per language/workspace
                                                ▼
                                         LspClient + DocumentStore
                                                │ JSON-RPC 2.0 over stdio
                                                ▼
                                      managed/local/custom child process
                                      stdin = protocol, stdout = protocol
                                      stderr = bounded diagnostic log only
```

`LspManager`는 workspace root와 현재 열린 문서를 기준으로 lazy session을 만든다. 같은
언어·같은 workspace에는 하나의 server process를 재사용하고, 다른 workspace는 별도
process를 갖는다. 첫 문서가 열릴 때만 활성화된 manifest를 실행하며, 서버가 설치되지
않았거나 사용자가 LSP를 끈 경우 process를 만들지 않는다. Tauri command는 짧은
상태 조회·설정 변경·명시적 install/start/stop만 담당하고, 오래 걸리는 process IO는
Rust async task가 담당한다.

권장 command/event 계약은 다음과 같다.

| 방향 | 계약 | 의미 |
|---|---|---|
| UI → Rust | `lsp_catalog`, `lsp_installed`, `lsp_configure` | 카탈로그·설치 metadata 조회, 서버 유형/활성화 설정 |
| UI → Rust | `lsp_install(manifest_id, version)` | 사용자가 확인한 exact manifest만 설치; `version`이 manifest와 다르면 거부 |
| UI → Rust | `lsp_start(language_id)`, `lsp_stop(language_id)` | 명시적 수동 시작/중지; lazy start는 파일을 편집할 때만 설정이 이미 enabled인 경우 |
| UI → Rust | `lsp_document_open|change|close`, `lsp_request` | document sync 및 기능 요청; request에는 client request id와 document version 포함 |
| Rust → UI | `lsp/status` | starting/ready/degraded/stopped/crashed, server metadata, backoff, capability |
| Rust → UI | `lsp/diagnostics` | URI, document version, diagnostics, stale 여부 |
| Rust → UI | `lsp/response` | request id, result/error, response document version |
| Rust → UI | `lsp/stderr` (debug log) | 사용자 동의/로그 레벨에 따른 최근 stderr; protocol data와 분리 |

프론트는 서버의 arbitrary JSON-RPC를 직접 실행하지 않는다. `LspAdapter`가 CodeMirror
위치와 DocId를 LSP URI·Position으로 바꾸고, Tauri event의 response id를 pending
completion/hover/etc.와 연결한다. 서버가 없어도 adapter는 no-op/fallback 구현을
제공한다.

### 프로세스와 JSON-RPC transport

LSP base protocol의 framing을 그대로 사용한다. writer는 UTF-8 JSON 한 개를 직렬화한
뒤 `Content-Length: <UTF-8 byte length>\r\n\r\n` 헤더와 정확히 그 byte 수의 body를
stdout에 쓴다. reader는 `\r\n\r\n`까지 header를 읽고, header name은
case-insensitive로 처리하되 `Content-Length`를 양의 정수로 반드시 확인한 후
`read_exact`한다. body를 줄 단위로 분할하거나 Unicode scalar 수로 길이를 세지 않는다.
빈 body, 중복/음수 Content-Length, 최대 message size(기본 16 MiB)를 넘는 frame,
잘못된 UTF-8/JSON은 protocol error로 session을 degraded 처리한다. 서버가 보내는
`Content-Type`은 검증 가능한 `application/vscode-jsonrpc; charset=utf-8` 또는
생략만 허용하고, 알 수 없는 header는 보존하지 않고 로그에 남긴다.

`Command::new(executable).args(args)`로만 child를 만든다. `cmd.exe /C`, PowerShell,
shell interpolation은 사용하지 않는다. managed entrypoint는 설치 디렉터리 안의
manifest-relative path인지 확인하고, local/custom executable은 canonical path를
저장해 실행 때 다시 존재·파일 여부를 확인한다. `current_dir`은 workspace root로
고정하고 환경 변수는 `PATH`, 언어 서버가 요구하는 명시적 runtime 변수, 사용자가
허용한 항목만 전달한다. 비밀 환경 변수 전체를 서버에 복사하지 않는다.

child의 stdin/stdout/stderr를 모두 pipe한다. stdout에는 protocol writer 외의 로그를
한 바이트도 섞지 않는다. stderr reader는 line/byte stream을 bounded ring buffer
(기본 64 KiB)로 수집하고 `log::debug!/warn!`와 선택적인 `lsp/stderr` event로만
전달한다. ring buffer가 차면 오래된 내용을 버리고 process를 죽이지 않는다. Windows
에서는 창을 만들지 않고 child process tree를 Job Object로 묶어 stop/crash 시 자식
Node process도 남지 않게 한다.

request id는 session마다 증가하는 `u64`이며 response가 오면 pending map에서 꺼낸다.
notification은 response를 기다리지 않는다. `initialize` 전에는 document request를
보내지 않고, `shutdown` response를 기다린 뒤 `exit` notification을 보낸다. graceful
shutdown이 2초 안에 끝나지 않으면 process tree를 강제 종료한다.

### 초기화·capability 협상

client는 LSP initialize request에 다음을 보낸다.

- `processId`, `clientInfo` (`code-pad`, 앱 버전), `rootUri`와 단일 `workspaceFolders`
  (둘 다 canonical workspace root를 가리킴)
- `capabilities.workspace.workspaceFolders`, `didChangeConfiguration`,
  `applyEdit`; `textDocument` 아래 synchronization, completion, hover, definition,
  references, rename, formatting 관련 client capability
- `general.positionEncodings: ["utf-16", "utf-8"]` (구현한 순서대로 선호)
- 서버별 `initializationOptions`는 manifest가 명시한 JSON object만 전달하고,
  custom 서버가 임의의 파일 읽기를 요구하면 UI에서 명시 확인한다.

서버 응답의 `capabilities`는 `serde(untagged)` 타입으로 bool/object 두 형태를 모두
읽는다. `textDocumentSync`, `completionProvider`, `hoverProvider`,
`definitionProvider`, `referencesProvider`, `renameProvider`,
`documentFormattingProvider`, `diagnosticProvider`를 실제 capability set으로
정규화하고, capability가 false/없으면 해당 UI 기능을 호출하지 않는다. 서버가
dynamic registration(`client/registerCapability`)을 보내면 등록된 method를 set에
추가하고 `unregisterCapability`로 제거한다. unsupported method에 대한 호출은
실패로 알리지 않고 fallback/disabled 상태로 표시한다. 서버가 반환한
`positionEncoding`을 session에 고정한다. 서버가 협상을 응답하지 않는 구형 구현은
LSP 호환 기본값인 UTF-16으로 시작하고 status에 `legacy position encoding`을 표시한다.

`initialize` timeout은 10초다. 성공 뒤 `initialized` notification을 보내고 모든
열린 문서를 순서대로 `didOpen`한다. 실패하면 process를 unavailable로 표시하고
editor는 즉시 Phase 1 상태로 돌아간다.

### Windows URI와 position 변환

LSP 위치는 파일 byte offset이 아니라 `line`과 `character`이며, server가 선택한
encoding을 따른다. CodeMirror 내부 문서는 Phase 1 불변식대로 LF와 JavaScript UTF-16
index를 사용하므로 다음 변환을 한 모듈에서만 구현한다.

- Windows `Path`는 canonicalize한 뒤 `Url::from_file_path`로 `file:///C:/...` 또는
  UNC URI를 만든다. drive letter 대소문자, backslash, percent encoding, UNC share를
  문자열 치환으로 직접 조립하지 않는다. URI → Path 변환도 `Url::to_file_path` 후
  canonicalize한다.
- `utf-16`이면 한 줄의 UTF-16 code unit 수를 계산한다. emoji/서로게이트 쌍은
  character 두 개이며, Rust `char` count나 UTF-8 byte count로 대신하지 않는다.
- `utf-8`이면 code point가 아닌 UTF-8 byte offset을 LSP character로 사용한다.
  편집기가 서버로 보내는 위치는 항상 유효 boundary에서 생성한다. 서버가 반환한
  `Position`이 중간 byte/code unit을 가리키면 diagnostic 표시만 가장 가까운 유효
  boundary로 clamp하고 로그에 남긴다. text edit, rename, formatting처럼 내용을
  바꾸는 응답은 임의 보정하지 않고 해당 edit 전체를 거부한다.
- CRLF는 buffer에서 이미 LF로 정규화돼 있으므로 LSP line은 `\n` 기준이다. 저장 시
  CRLF로 다시 바꾸는 것은 LSP 위치 계산 뒤의 file command가 담당한다.
- 서버가 반환한 range가 현재 line/text 범위를 벗어나면 diagnostic 표시만 clamp하고,
  edit는 전체 작업을 거부한다. 위치 변환 테스트에는 `a😀b`, 한글, 빈 줄, CRLF 원본,
  multi-code-unit rename을 반드시 포함한다.

### workspace 및 document sync

LSP session의 경계는 현재 code-pad의 작업 폴더 하나다. root를 정한 뒤 모든 document
path는 canonical path가 root 아래인지 확인한다. Windows drive/UNC 비교는
case-insensitive한 `Path` component 비교를 사용하고, `..`, junction, symlink를
확인한 real path가 root 밖이면 거부한다. workspace 밖에 있는 dependency의
definition/reference location은 결과 목록에서 제외하거나 "workspace 밖이라 열 수
없음"으로 표시한다. 사용자가 별도 폴더를 작업 폴더로 열면 기존 session을 멈추고
새 workspace session을 만든다. 다만 로컬 LSP process는 사용자 계정의 파일 권한으로
실행되므로 클라이언트가 서버 자체의 임의 파일 읽기를 sandbox로 차단한다고 약속할 수
없다. 설치·활성화 화면에서 이 trust boundary를 알리고, code-pad가 서버에 보내거나
서버 결과로 여는 URI만 workspace 내부로 제한한다.

문서마다 `{ uri, language_id, version, text, dirty }`를 유지한다.

1. `didOpen`은 session당 URI 한 번만 보내며, `version = 1`과 현재 LF text를 full
   content로 보낸다. 같은 URI를 다른 workspace session에 재사용할 수는 있지만 각
   session의 version은 독립적이다.
2. CodeMirror transaction마다 version을 1 증가시킨다. 서버가
   `TextDocumentSyncKind::Incremental`을 선택했으면 transaction 전후 text의 공통
   prefix/suffix를 기준으로 바뀐 최소 연속 range 하나를 계산한다. range는 변경 전
   document 기준 UTF-16/UTF-8 위치이고 replacement는 변경 후 text의 해당 구간이다.
   이 방식은 여러 cursor와 여러 줄 변경도 순서 의존적인 복수 edit 없이 정확히 한
   `contentChanges` 항목으로 표현한다. Full이면 새 전체 text 하나를 보낸다. 전송은
   version 순서대로 직렬화한다.
3. `didChange`에 version을 포함한다. pending response에는 요청 시점 version을
   함께 저장해 현재 version보다 낮은 결과를 버린다. diagnostics도 URI와
   `version`이 일치하지 않으면 stale로 표시하고 새 결과가 올 때까지 화면의 최신
   진단을 덮어쓰지 않는다.
4. `didSave`는 실제 `saveFile` 성공 뒤에만 보낸다. 외부 변경을 reload하면 local
   document version을 새로 시작하지 않고 1 증가시켜 full `didChange`를 보내며,
   server가 저장 후 상태를 다시 계산하게 한다. 사용자가 "유지"를 선택한 dirty
   buffer는 디스크 snapshot과 무관하게 계속 sync한다.
5. `didClose`는 문서가 마지막 탭에서 닫힐 때 보내고, process restart 뒤에는 열린
   문서를 모두 `didOpen`으로 재동기화한다. server가 죽거나 timeout돼 재시작할 때
   프론트 buffer와 undo history는 건드리지 않는다.

### 기능별 요청과 결과 적용

| 기능 | LSP method | 적용 규칙 |
|---|---|---|
| 진단 | server notification `textDocument/publishDiagnostics` | URI/version을 확인한 뒤 severity/range를 CodeMirror lint로 변환. 최신 version만 표시 |
| 자동완성 | `textDocument/completion` (+ optional resolve) | cursor position과 current version을 보내고 다음 입력 시 취소. 실패/미지원이면 단어 완성 유지 |
| hover | `textDocument/hover` | 현재 position에 해당하는 markdown/plaintext만 표시; timeout이면 닫힘 |
| 정의 | `textDocument/definition` | workspace 내부 Location만 탭/Quick Open으로 연결 |
| references | `textDocument/references` | workspace 내부 결과만 목록에 표시; 결과는 요청 version snapshot과 함께 보관 |
| rename | `textDocument/rename` | WorkspaceEdit를 preflight하여 모든 URI·range·version을 검증한 뒤 한 번에 editor buffers에 적용. 자동 저장하지 않음 |
| formatting | `textDocument/formatting` | 사용자가 명시적으로 실행할 때만 TextEdit 적용; 범위를 벗어난 edit는 거부 |

WorkspaceEdit는 문서별 현재 version과 비교한 뒤 충돌하면 전체 적용을 중단하고
사용자에게 재시도하도록 한다. 한 파일만 일부 적용해 rename을 망가뜨리지 않는다.
server의 custom command/code action은 Phase 2 API에 노출하지 않으며, 위 표 밖의
method는 capability와 무관하게 무시한다. LSP server가 `diagnosticProvider`만 광고하면
`textDocument/diagnostic` pull을 Phase 2에서 구현해 열기·변경·저장 뒤 debounce하여
호출한다. push와 pull을 모두 제공하면 version이 확인되는 최신 결과 하나만 화면에
적용하고 중복 진단은 합치지 않는다.

### install/update 보안과 원자성

managed installer는 `reqwest`로 HTTPS artifact를 streaming download하고 다음 순서를
지킨다.

1. 사용자가 catalog의 exact `id`와 `version`을 선택한다. catalog에 없는 version,
   플랫폼, URL host, license/source 누락은 요청 단계에서 거부한다. redirect의 최종
   host도 manifest allowlist와 다르면 거부한다.
2. `%LOCALAPPDATA%\\com.workbench.codepad\\lsp\\downloads\\<nonce>.part`에 쓰고
   응답 크기·manifest `size_bytes`·최대 설치 크기를 검사한다. 취소/네트워크 실패는
   `.part`를 설치 경로로 승격하지 않는다.
3. 스트림을 끝까지 읽은 뒤 SHA-256을 계산해 manifest의 lowercase digest와
   constant-time 비교한다. 불일치하면 파일을 폐기하고 기존 설치는 그대로 둔다.
   npm registry의 integrity(SHA-512)가 함께 있는 경우에도 최종 설치 계약은
   SHA-256 필드를 반드시 검증한다.
4. archive는 별도 nonce staging directory에만 푼다. zip/tar/gzip entry마다
   archive-relative path를 검사해 `/x`, `\\x`, drive prefix(`C:`), 빈 component,
   `..`를 거부하고, canonicalized destination이 staging root 아래인지 매 entry마다
   재확인한다. symlink, hardlink, device/fifo entry는 Phase 2에서 모두 거부한다.
   `archive_root` 밖의 executable이나 manifest가 기대한 entrypoint와 다른 파일은
   설치하지 않는다. 이 검사는 zip slip과 Windows path traversal을 모두 막는다.
5. staging에서 entrypoint 존재·파일 종류·manifest version·license/source metadata와
   dependency lock digest를 다시 검사하고, Node package의 모든 lock entry가 exact
   URL/digest를 만족하는지 확인한다. 실행 파일은 검증 전 절대 실행하지 않는다.
6. 검증된 staging을 `servers/<id>/<version>/<platform>/`의 새 immutable directory로
   rename한다. 활성 pointer/installed index는 `index.json.tmp-<nonce>`에 JSON을
   쓰고 flush한 뒤 같은 디렉터리 안에서 atomic replace한다(Windows에서는
   `ReplaceFileW`/동등한 atomic replace primitive를 사용하고, 기존 파일을 먼저
   삭제하지 않는다). 기존 version을 덮어쓰지 않으며, pointer 교체가 성공한 뒤에만
   이전 version을 사용자가 요청한 경우 삭제한다. crash가 어느 단계에서 나도 이전
   index와 active install은 유효해야 한다.

설치·업데이트는 자동 실행되지 않는다. 앱 시작 때 index만 읽고, catalog version이
달라져도 update badge를 표시할 뿐 다운로드하지 않는다. 사용자가 Update를 누르면
새 manifest의 version/digest/license/source를 다시 확인하고 위 절차를 거친다.
기존 local/custom server는 이 installer를 거치지 않으며 경로·args·runtime을
설정에서 명시적으로 보여 준다. managed package의 uninstall도 사용자가 확인한
경우에만 실행하고 활성 session은 먼저 stop한다.

### 상태 저장

기존 Phase 1의 `session.json`에는 LSP process object나 binary content를 넣지 않는다.
다음 두 파일을 `app_local_data_dir()` 아래에 둔다.

```text
session.json       # 열린 문서, view, cursor, bookmark, workspace (기존 schema)
lsp/config.json    # 사용자가 선택한 server source/id/version/enable 상태
lsp/installed.json # managed install metadata와 exact digest/entrypoint
```

`lsp/config.json`은 schema `version`을 가지며 다음 필드를 저장한다.

```text
LspConfig {
  version: 1,
  enabled: bool,
  workspace_root: canonical path,
  server_by_language: { language_id: ServerRef },
  custom_servers: [{ language_ids, executable, args, runtime, source, license, version }],
  update_policy: "manual",
}
ServerRef { kind, manifest_id?, version?, installed_path?, executable?, args? }
```

`lsp/installed.json`의 각 entry는 `manifest_id`, exact `version`, `platform`,
`sha256`, `source_url`, `license`, `artifact_url`, `installed_path`, `entrypoint`,
`runtime`, `installed_at`, `package_lock_sha256`, `install_source`,
`last_verified_at`를 저장한다. `install_source`는 `network`, `archive_cache`,
`local_archive`, `unknown` 중 하나이며 선택한 local archive의 원본 경로는 저장하지 않는다.
검증된 compressed artifact는 `lsp/downloads/cache/<sha256>.<ext>`에만 보관하고
Node local import는 reviewed dependency closure의 `.tgz` 경로 목록을 native lock에 매칭해
missing/duplicate/extra archive를 거부한다. 설치 경로는 app data 아래
versioned directory로 canonicalize할 수 있어야 하며, index metadata와 실제 파일이
불일치하면 installed가 아닌 `needs reinstall`로 표시한다. session 손상과 동일하게
schema version이 맞지 않거나 JSON이 깨지면 empty LSP config로 시작하되, 파일을
자동으로 보정해 덮어쓰지는 않고 오류를 로그에 남긴다.

process 상태와 pending request는 메모리에만 둔다. 앱 재시작 시 server를 복원하지
않고, 사용자가 enable한 서버가 있으면 첫 문서에서 다시 시작한 뒤 현재 열린 문서를
`didOpen`한다. 서버의 새 session에서는 document version을 1부터 다시 시작한다;
editor의 dirty/undo/session state는 유지한다. `stderr` ring buffer와 crash count는
재시작 시 초기화하되 현재 run의 diagnostics log에는 남긴다.

### 실패·취소·재시작 정책

| 상황 | 처리 |
|---|---|
| catalog/lock에 없는 version 또는 source/license 누락 | install 전 거부; 기존 설치와 editor는 영향 없음 |
| HTTPS/redirect/size/checksum 실패 | staging/partial만 폐기, 기존 active version 유지, 사용자가 Retry할 때만 재시도 |
| archive traversal/link/entrypoint 검증 실패 | install 실패로 표시; 어떠한 파일도 active로 승격하지 않음 |
| Node/native runtime 또는 executable 없음 | 해당 language session만 unavailable; CodeMirror 기능은 계속 사용 |
| initialize 실패/잘못된 framing/invalid JSON | process 종료 후 status degraded; 마지막 diagnostics는 stale 표시하고 자동 기능 비활성 |
| request timeout | `$/cancelRequest`를 보내고 pending을 error로 완료. completion/hover 2초, definition/references/rename/formatting 5초, initialize 10초 |
| 새 입력이 도착한 completion/hover | 이전 request를 취소하고 current document version만 적용 |
| server crash/비정상 exit | stderr와 exit code를 기록하고 현재 버퍼는 보존. 짧은 자동 재시작 backoff 후 문서 재동기화 |
| 반복 crash | 1s, 2s, 4s, 8s, 최대 30s의 exponential backoff; 5분 안에 3회 실패하면 자동 재시작 중지, 명시적 Restart만 허용 |
| server stop | `shutdown`→2초 대기→`exit`→필요하면 process tree kill; pending 요청은 cancelled |
| workspace 밖 URI/path 또는 malformed edit | 해당 result/edit만 거부하고 보안 로그에 남김; 전체 session은 유지 |
| stale diagnostics/response | 현재 document version보다 낮으면 화면에 적용하지 않음 |
| `WorkspaceEdit` 중 한 문서 충돌 | 모든 edit를 preflight에서 폐기하고 partial rename/formatting을 하지 않음 |

자동 재시작은 사용자가 이미 활성화한 managed/local/custom server에만 적용하며,
다운로드나 version 변경을 포함하지 않는다. backoff가 끝난 뒤에도 process가 죽으면
status banner에서 `Restart server`를 제공한다. 사용자가 LSP를 끄면 process를 즉시
stop하고 이후 파일 편집은 서버와 무관하게 작동한다.

### Phase 2 테스트와 검증

네트워크와 실제 catalog artifact에 의존하는 테스트를 CI의 기본 경로로 만들지 않는다.
installer는 고정된 local fixture archive와 fake HTTP response를 사용하고, 실제
release/package metadata의 검증은 catalog update 작업에서 별도로 수행한다.

- **Rust transport/process**: header와 body가 여러 read로 분할되는 경우, multi-byte
  UTF-8 body의 byte length, malformed/oversized frame, 여러 request의 response 순서,
  notification, stderr ring buffer, shutdown timeout, `$/cancelRequest`, crash
  backoff를 테스트한다. fake LSP child는 stdin/stdout만 사용해 `initialize`,
  `didOpen`, `didChange`, `publishDiagnostics`, completion을 deterministic하게
  응답한다.
- **Rust protocol/document**: capability bool/object와 dynamic registration,
  unsupported feature fallback, URI round-trip(`C:\\work`, UNC, percent-encoded name),
  UTF-16 surrogate pair/한글/emoji, UTF-8 byte position, CRLF→LF, monotonic document
  version, stale response/diagnostic discard를 테스트한다.
- **Rust security/install**: wrong SHA-256, interrupted download, size limit,
  absolute/drive/`..` archive names, symlink/hardlink/device entries, symlinked staging
  path, wrong entrypoint, dependency lock mismatch, atomic index replacement과 crash
  recovery를 테스트한다. fixture에서 검증 전 entrypoint가 실행되지 않았음을
  확인한다.
- **Vitest**: server 미설치/disabled에서 CodeMirror editor·word completion·save가
  정상인 경우, capability별 UI 노출, diagnostics mapping, completion cancellation,
  stale response, definition/references workspace filtering, rename/formatting
  preflight, install confirmation과 version/license/source/SHA 표시를 테스트한다.
- **Windows 수동 smoke**: 검증된 rust-analyzer와 Node 서버를 사용자가 직접 설치한
  뒤 실제 Windows path/UTF-16 위치, Rust/TS/Python/JSON·HTML·CSS의 diagnostics와
  completion/hover/definition/references/rename/formatting, server stop/restart,
  workspace 밖 definition rejection을 확인한다. 네트워크 없는 CI에서는 이 단계를
  실행하지 않는다.
- **저장소 gate**: implementation PR마다 `cargo test`, `cargo check`, `pnpm build`,
  `pnpm test`와 기존 Windows `cargo test --workspace`를 통과시키며, Phase 2 설계
  변경 자체는 문서 파일만 포함해야 한다.

### 구현 순서

Phase 1의 두 prerequisite PR( filesystem 제한 순회와 `crates/markdown` 추출)이 먼저
main에 들어간다는 전제를 유지한다. 이후에도 기능별 PR 하나에 하나의 책임만 둔다.

1. **앱 로컬 경계와 schema** — `apps/code-pad/src-tauri/src/lsp/` 모듈,
   `LspConfig`/`InstalledServer`/`ServerManifest`, 초기 catalog fixture, UI의
   metadata/explicit-confirmation 상태를 먼저 만든다. `crates/lsp`는 만들지 않는다.
   JSON schema 왕복·version mismatch 테스트와 `cargo check`를 통과시킨다.
2. **stdio transport/process** — no-shell Windows child spawn, Content-Length framing,
   request map, stderr ring, graceful stop, timeout/cancel, fake-server fixture를
   구현한다. framing/process Rust tests와 Windows compile check를 통과시킨다.
3. **URI/position/document store** — workspace boundary, Windows file URI,
   negotiated position encoding, document version/full/incremental sync, stale result
   rules를 구현한다. emoji/CRLF/UNC/escape tests를 통과시킨다.
4. **initialize/capability** — initialize/initialized, static/dynamic capability
   normalization, status events와 no-server fallback을 붙인다. fake server capability
   matrix와 `pnpm build`를 통과시킨다.
5. **기능 adapter** — diagnostics와 completion부터 연결한 뒤 hover, definition,
   references, rename, formatting 순서로 붙인다. WorkspaceEdit preflight와
   cancellation을 포함하고 CodeMirror/vitest를 기능별로 추가한다.
6. **managed installer** — catalog exact manifest, dependency lock, HTTPS allowlist,
   streaming SHA-256, safe archive extraction, atomic install/index, explicit update/
   uninstall UI를 구현한다. 실제 네트워크 없이 fixture 보안 tests를 통과한 뒤,
   Windows에서 위 검증 artifact의 manual install을 수행한다.
7. **local/custom runtime** — existing path 검사, Node runtime 선택, argv-only
   custom stdio 등록, source/license/version unknown metadata와 workspace cwd를
   연결한다. shell injection/path escape 테스트를 통과시킨다.
8. **복원·운영성** — `lsp/config.json`/`installed.json` 복원, lazy session, crash
   backoff, status/log UI, stale diagnostics banner를 붙인다. 재시작 중 dirty buffer·
   undo history가 유지되는 component/integration tests를 통과시킨다.
9. **최종 검증** — CI 전체 gate와 Windows manual smoke를 실행하고, catalog의
   version/license/source/digest가 실제 metadata/release asset과 일치하는지 기록한다.
   이 문서는 구현 PR에서 변경된 command/schema와 함께 동기화한다.
10. **두 번째 소비자 발생 시에만 추출** — 다른 앱이 같은 LSP transport/client를
    실제로 import하는 PR이 생긴 때에 한해 `apps/code-pad/.../lsp`의 공통 부분을
    `crates/lsp`로 옮긴다. 추출 PR에는 두 앱의 tests와 workspace member/path
    dependency를 포함하며, 그 전에는 선제적으로 공용 crate를 만들지 않는다.

### 의존성 및 공식 사실 확인 기록

#### Phase 2 의존성

- Rust LSP core: `lsp-types`(LSP 3.17 data types), `serde`/`serde_json`, `url`(file URI
  round-trip), `sha2`(SHA-256), `reqwest`(HTTPS streaming; 기존 workspace의 reqwest
  사용 패턴과 맞추되 code-pad `Cargo.toml`에 직접 선언), `zip`, `tar`, `flate2`(검증된
  archive fixture), `tempfile`(test only). async child IO는 Tauri가 제공하는
  `tauri::async_runtime`를 우선 사용하고, 직접 tokio API가 필요할 때만 `tokio`를
  직접 의존한다. Windows Job Object와 no-window spawn은 `windows` crate를
  `cfg(windows)` 명령 계층에만 둔다.
- Rust protocol reference: [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
  의 base protocol framing, initialize, text document synchronization, position
  encoding, WorkspaceEdit 계약을 따른다. 표준에 없는 서버별 request는 catalog
  adapter로 격리한다.
- Frontend: 기존 Phase 1의 CodeMirror 직접 import 목록(`codemirror`,
  `@codemirror/state`, `@codemirror/view`, `@codemirror/commands`,
  `@codemirror/language`, `@codemirror/search`, `@codemirror/autocomplete`,
  `@codemirror/lang-*`)과 LSP adapter용 TypeScript types. Tauri event/invoke를
  직접 호출하는 파일은 `src/api.ts`에 두고 컴포넌트에는 typed wrapper만 노출한다.
- Managed Node servers: Node는 code-pad에 번들하지 않는다. catalog의 `runtime`
  조건을 만족하는 시스템 Node 또는 사용자가 고른 Node executable을 사용하고,
  npm package root와 transitive dependency를 exact lock manifest로 설치한다. npm
  semver range를 설치 시점에 해석하지 않으며, reviewed lock이 없는 패키지는
  managed catalog에 넣지 않는다.

#### 공식 사실 확인 기록 (2026-08-12)

다음 명령과 primary source로 catalog 표의 version, license, repository, artifact,
entrypoint, runtime, checksum을 확인했다. 이는 구현 시 다시 실행해야 하는 갱신
절차이지, 서버 version을 자동으로 추적하는 런타임 동작이 아니다.

```text
npm view typescript-language-server@5.3.0 version license repository dist.tarball dist.integrity bin engines --json
npm view typescript@6.0.3 version license repository dist.tarball dist.integrity engines --json
npm view basedpyright@1.39.9 version license repository dist.tarball dist.integrity bin engines --json
npm view vscode-langservers-extracted@4.10.0 version license repository dist.tarball dist.integrity bin engines --json
gh api repos/rust-lang/rust-analyzer/releases/latest
```

조회 결과에서 TypeScript server는 `typescript-language-server` bin, `--stdio`, Node
`>=20`, Apache-2.0 metadata를 제공하고 upstream [README의 Installing/Running
절](https://github.com/typescript-language-server/typescript-language-server#installing)은
TypeScript package를 함께 설치하도록 한다. basedpyright npm metadata는
`basedpyright-langserver`와 `pyright-langserver` 두 bin, Node `>=14.0.0`, MIT를
제공하며 공식 [language-server 문서](https://docs.basedpyright.com/latest/installation/command-line-and-language-server/)
와 [IDE 설정 예](https://docs.basedpyright.com/latest/installation/ides/)는
`basedpyright-langserver --stdio`를 사용한다. vscode-langservers-extracted metadata는
JSON/HTML/CSS bin과 MIT, 그리고 별도 dependency 목록을 제공하므로 reviewed lock이
필요하다는 결론을 냈다. rust-analyzer 공식 latest release API는 2026-08-10.1의
Windows x64 zip asset과 digest를 제공하며, 설치 전에 그 exact asset만 사용한다.

실제 다운로드한 네 개의 root artifact에 대해 SHA-256을 계산한 값은 카탈로그 표에
기록했다. npm registry가 제공하는 SHA-512 integrity는 보조 검증으로 저장하되,
installer 계약의 필수 값은 표의 SHA-256이다. source/license/artifact가 나중에
바뀌면 기존 installed version은 유지하고, 새 manifest와 새 checksum을 검토한
명시적 update만 허용한다.

#### Phase 1 의존성

- Rust: `encoding_rs = "0.8"`, `chardetng = "1"`, `notify = "8"`(everything-plus의
  `Cargo.toml:25`와 동일 계열 메이저 버전 — 이미 이 저장소 Cargo 워크스페이스에
  8.x가 들어와 있으므로 맞춰 둔다), `serde`/`serde_json`.
- 프론트: `codemirror` 6과 직접 import하는 `@codemirror/state`,
  `@codemirror/view`, `@codemirror/commands`, `@codemirror/language`,
  `@codemirror/search`, `@codemirror/autocomplete`, `@codemirror/lang-*` 언어팩,
  `mermaid`(프리뷰 — knowledge-base가 이미 `^11.16.1`을 쓰고 있다,
  `apps/knowledge-base/package.json:16`). 이 패키지들은 모두
  `apps/code-pad/package.json`에 직접 선언한다. `codemirror`가 일부를 transitive
  dependency로 끌어오더라도 pnpm importer에서 직접 import할 수 있다는 뜻은
  아니므로, `@codemirror/search`를 포함한 직접 import 목록과 lockfile을 함께
  갱신한다. pnpm workspace에서 mermaid의 동일 계열 버전을 맞추면 중복 설치를
  줄일 수 있지만, 정확한 버전 고정은 code-pad 구현 시점에 최신 안정판을 확인해
  정한다.

`encoding_rs`/`chardetng`은 저장소 어떤 `Cargo.toml`에도 아직 없다(확인:
`grep -rln "encoding_rs\|chardetng" apps/*/src-tauri/Cargo.toml Cargo.toml` 결과
없음) — code-pad가 첫 도입이다.
