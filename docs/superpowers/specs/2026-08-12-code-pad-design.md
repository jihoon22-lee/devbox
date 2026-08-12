# code-pad — 경량 코드 에디터 설계

- 날짜: 2026-08-12
- 브랜치: `docs/code-pad/design-spec`
- 범위: 신규 앱 `apps/code-pad`의 설계. **이 문서 자체가 산출물이며 구현은 하지 않는다.**
- 전제: `crates/filesystem`, `crates/markdown` 추출 PR 2개가 code-pad 구현 PR보다 먼저 머지되어야
  한다. 근거는 "공통 추출"과 "구현 순서" 절에 있다.

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

**Phase 2**: LSP(`crates/lsp`, Windows 로컬 경로 한정). 이 문서는 Phase 1 설계이므로
LSP의 인터페이스는 정하지 않는다.

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
| `crates/filesystem` | everything-plus (사용 중) | Ctrl+P 폴더 순회 | 지금 추출 — 두 번째 소비자 도달 |
| `crates/markdown` | knowledge-base (사용 중) | 프리뷰 패널 | 지금 추출 — 두 번째 소비자 도달 |
| `packages/editor` | code-pad (최초) | — | **추출 안 함** — 첫 소비자 |

근거를 코드로 확인한 내용:

- **`crates/filesystem`**: everything-plus의 실제 폴더 순회는 `core/indexer.rs`의
  `collect()`(`indexer.rs:14-39`)가 `walkdir::WalkDir`로 순회하며
  `core/ignore.rs`의 `is_ignored_dir()`(`ignore.rs:1-24`, `.git`/`node_modules`/
  `target` 등 18개 이름 매치)로 디렉터리를 걸러낸다. code-pad의 Ctrl+P가 같은 walk+
  ignore 조합을 두 번째로 실제 사용하므로, 지금이 "두 번째 소비자가 필요해진" 시점이다.
  추출 PR에서는 everything-plus의 무제한 인덱싱과 code-pad의 제한된 빠른 열기를 모두
  지원하도록 공통 API를 나눈다. 구체적으로 `walk_files(root, max_entries)`가 파일
  경로·크기·mtime 목록과 `truncated`를 반환하고, `max_entries`에 도달하면 Rust
  순회 루프에서 즉시 중단한다. everything-plus는 기존처럼 제한 없이 호출하고,
  code-pad는 `50_000`을 넘긴다. 기존 `IndexedFile`(크기·mtime을 담은 인덱싱 특화
  구조체, `indexer.rs:6-11`)를 그대로 빠른 열기 API로 노출하지는 않는다.
- **watcher는 옮기지 않는다**: everything-plus의 `Cargo.toml:26`에 `notify = "8.2.0"`이
  선언돼 있지만, `apps/everything-plus/src-tauri/src/` 전체에서 `notify::`를 참조하는
  코드가 **0건**이다(확인: `grep -rn "notify::" apps/everything-plus/src-tauri/src`).
  즉 notify/watcher는 실제 소비자가 code-pad 하나뿐이라 추출 기준(두 번째 소비자)을
  만족하지 못한다. `watcher.rs`는 code-pad 안에 남는다.
- **`crates/markdown`**: knowledge-base 설계 문서의 결정 3이 이미 이 판단을 내려
  뒀다 — "`crates/markdown`으로 선제 추출하지 않는다 ... code-pad 차례가 오면
  `apps/knowledge-base/src-tauri/src/core/markdown.rs`를 `crates/markdown/`으로
  옮기고 루트 `Cargo.toml`의 `workspace.members`에 추가하는 ... 절차를 따른다"
  (`docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:50-65`).
  code-pad가 프리뷰 패널에서 이 렌더러를 쓰는 지금이 그 "차례"다. 현재 파일의
  테스트 첫 항목은 `crate::core::frontmatter`를 가져오므로
  (`apps/knowledge-base/src-tauri/src/core/markdown.rs:188-203`), generic crate로
  옮길 때는 frontmatter가 이미 제거된 body를 직접 넘기는 테스트로 바꾼다.
  frontmatter 제거와 이미지 경로 해석은 각 앱의 command 레이어 책임으로 남긴다.
- **`packages/editor`는 추출하지 않는다**: code-pad가 CodeMirror의 첫 소비자다(배경
  절 참고). CONVENTIONS §4의 추출 기준 문구 그대로 "같은 도메인 코드가 **두 번째
  앱**에서 필요해지면"(`CONVENTIONS.md:137-139`) 옮기므로, 첫 소비자 단계에서는
  `apps/code-pad/src/editor/`에 코드를 둔다. knowledge-base가 나중에 CodeMirror로
  옮겨 오는 시점이 두 번째 소비자다.

두 crate 모두 아직 존재하지 않는다 — `find crates`/`find packages`가 디렉터리 자체를
찾지 못하고(`No such file or directory`), 루트 `Cargo.toml`의 `members` 목록도
현재 10개 앱의 `src-tauri`만 나열하고 있으며 주석 처리된 예시 3개(`crates/process`,
`crates/wsl`, `crates/database`, `Cargo.toml:16-18`)에는 `filesystem`도 `markdown`도
없다. CONVENTIONS §2의 저장소 구조 스케치(`CONVENTIONS.md:61-67`)는 `crates/filesystem`의
예상 소비자로 "everything-plus, knowledge-base, life-log"를 적어 뒀지만, 이는 저장소
초기 설계 시점의 가상 표이고 code-pad는 물론 `markdown` crate 자체도 그 표에 없다.
실제 추출을 결정하는 근거는 이 가상 표가 아니라 지금 코드에 실재하는 소비자 수이며,
`packages/`는 이번에도 비어 있게 되고 `crates/`가 이 두 PR로 처음 채워진다.

## 모듈 구조

CONVENTIONS §4의 앱별 Rust 모듈 구조(`CONVENTIONS.md:126-140`, `core/`는 OS 비의존
순수 로직·WSL에서 `cargo test`)와 프론트엔드 구조(`CONVENTIONS.md:143-153`)를 그대로
따른다.

```
apps/code-pad/src-tauri/src/
  lib.rs              run(), command 등록, 앱 상태와 watcher manager 보관
  commands/{file,folder,preview,session}.rs
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
기동 또는 개별 등록에 실패하면 해당 등록만 비활성화하고 mtime+size 저장 검사를
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
치명적이지 않은 이유는 아래 저장 흐름의 mtime+size 비교가 2차 방어선이기 때문이다.

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

- **열기**: `openFile(path)` → metadata(크기·mtime) 조회 → 임계치 초과면
  `read_only` 확정 → 바이트 읽기 → `encoding::detect()` → 디코딩 →
  `line_ending::detect()` → LF 정규화 → `OpenedFile { text, encoding, line_ending,
  read_only, size, mtime }` 반환. `size`와 `mtime`은 프론트가 파일 snapshot으로
  함께 보관한다.
- **저장**: `saveFile(path, text, encoding, lineEnding, expectedMtime, expectedSize)`
  → 디스크의 현재 mtime 또는 크기가 각각의 expected 값과 다르면
  `Err(Conflict)` → LF를 `lineEnding`으로 변환 → `encoding`으로 인코딩 → 쓰기
  → 다시 metadata를 조회해 `SavedFile { mtime, size }`를 반환한다. 저장 성공 시
  프론트는 이 새 snapshot을 다음 저장의 expected 값으로 교체하고, 충돌 시에는
  기존 snapshot과 dirty 내용을 그대로 유지한다. 이 mtime+size 비교는 watcher가
  놓친 레이스(함정 ②)에 대한 2차 방어선이다 — watcher가 죽어 있어도(아래 에러 처리
  표 참고) 남의 변경을 덮어쓰지 않는다.
- **외부 변경**: watcher → 디바운스 → `app.emit("file-changed")` → 프론트가 그
  문서의 dirty 여부로 자동 리로드/배너를 분기한다(결정 4). 자동 리로드하거나
  사용자가 리로드를 선택하면 새 metadata snapshot도 함께 교체한다.
- **Ctrl+P**: 작업 폴더를 지정하면 `crates/filesystem`의
  `walk_files(root, 50_000)`으로 **1회** 목록을 만든다. 이 상한은 프론트에서
  자른 뒤가 아니라 Rust walk 루프에서 적용하며, 반환된 `truncated`가 true면
  "폴더가 커서 일부만 색인했습니다" 배너로 안내한다(거부하지 않는다 — 근거는
  "확정된 세부 결정" 2번). 타이핑에 따른 필터링은 전부 프론트에서 한다.
  watcher는 열려 있는 문서 전용이고 이 목록을 갱신하는 데 쓰지 않는다 — 폴더 전체를
  감시하는 비용을 지지 않는다.
- **세션**: 파일 열기·닫기·뷰 이동·활성 탭 변경마다 1초 디바운스로 `session.json`을
  저장한다.

### 프리뷰 흐름

`.md` 프리뷰는 `commands/preview.rs`의 `render_preview(path, content)`가 담당한다.
추출된 `crates/markdown`의 `render()`는 frontmatter가 제거된 본문을 받는 API이므로
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
| 큰 파일(5MB 초과) | 읽기 전용 + 하이라이팅 비활성, 상태바에 사유 표시(임계치 근거는 "확정된 세부 결정" 1번) |
| 인코딩 감지 실패 | UTF-8 lossy로 열고 표시 + 수동 인코딩 선택 제공(선택 목록은 "확정된 세부 결정" 5번) |
| 저장 시 인코딩 불가 문자 | **저장 중단** + UTF-8 전환 제안. 조용한 손실 금지(CP949 파일에 이모지를 넣는 실제 시나리오) |
| 저장 시 mtime/크기 충돌 | 저장 중단 + 배너. dirty 버퍼와 기존 expected snapshot은 유지 |
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
  손상 입력)
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
- LSP — Phase 2로 유보(`crates/lsp`, Windows 로컬 경로 한정). 이 문서는 인터페이스를
  정하지 않는다.
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

### 1. 큰 파일 임계치 → 5MB

**결정**: 5MB를 넘으면 읽기 전용 + 하이라이팅 비활성으로 연다.

**근거**: CodeMirror 6은 뷰포트 단위로만 DOM을 그리므로 문서 크기 자체는 큰
파일도 버티지만, 문법 하이라이팅을 담당하는 Lezer 증분 파서는 문서가 커질수록
파싱 비용이 늘어나 수 MB급 파일에서 입력 지연으로 체감된다. 이 상한을
everything-plus의 `MAX_CONTENT_BYTES`(1MB, `apps/everything-plus/src-tauri/src/core/indexer.rs:53`)나
knowledge-base의 이미지 인라인 2MB 상한(`docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:112`)과
다르게 잡는 이유: 그 두 값은 "인덱싱 대상 파일 전체" 또는 "문서 하나에 박힌
이미지 여러 개"에 반복 적용되는 **누적** 비용 기준이고, code-pad의 5MB는
"사용자가 지금 연 파일 하나"에 대한 **단발** 비용 기준이다 — 적용되는 횟수
자체가 다르므로 같은 값을 재사용할 근거가 없다.

### 2. Ctrl+P 목록 상한 → 50,000개, 초과 시 잘라내고 배너 안내

**결정**: `crates/filesystem`의 Rust walk에 `max_entries = 50_000`을 넘겨
순회 중 그 수에 도달하면 즉시 중단한다. 프론트에는 그때까지의 목록과
`truncated` 플래그만 전달하고, 그 이상을 거부하지 않는다. 잘렸으면 "폴더가 커서
일부만 색인했습니다" 배너로 안내한다.

**근거**: `crates/filesystem`으로 옮겨질 ignore 규칙이 `node_modules`/`target`/
`.git` 등 대용량 디렉터리를 이미 걸러낸다(everything-plus `core/ignore.rs:1-24`의
18개 이름 매치가 그대로 재사용된다). 일반적인 프로젝트 폴더는 이 필터를 거치면
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

### 8. expected file snapshot → epoch nanos `i64` + 파일 크기 병행 비교

**결정**: `SystemTime`을 epoch 나노초 기준 `i64`로 직렬화해 `expectedMtime`에
담고, 열 때 읽은 파일 크기를 `expectedSize: u64`로 함께 보관한다. 저장 커맨드는
`saveFile(path, text, encoding, lineEnding, expectedMtime, expectedSize)` 형태로
두 값을 모두 받아 디스크의 현재 mtime 또는 크기가 하나라도 다르면
충돌(`Err(Conflict)`)로 처리한다. 저장에 성공하면 커맨드가 실제로 다시 조회한
`{ mtime, size }`를 반환하고, 프론트는 이를 다음 저장의 새 expected snapshot으로
교체한다. 외부 변경을 리로드한 경우에도 같은 방식으로 snapshot을 갱신한다.

**근거**: 밀리초 단위로는 같은 밀리초 안에서 연속으로 파일이 쓰였을 때(예:
빌드 스크립트가 파일을 빠르게 여러 번 저장하는 경우) 서로 다른 두 저장을
같은 값으로 오인할 수 있다. 나노초 단위 `i64`는 1970년부터 약 292년(2262년경)
까지 표현 가능해 이 앱의 수명 안에서 오버플로를 걱정할 필요가 없다.

파일시스템별 mtime 정밀도 차이(NTFS는 100ns 단위, FAT류는 2초 단위)는 문제가
되지 않는다 — 비교 대상이 "절대 시각이 정확한가"가 아니라 "열 때 읽어서 들고
있던 값과 지금 디스크의 값이 같은가"이므로, 정밀도가 어느 수준으로 잘려도 열
때와 저장할 때 동일한 방식으로 잘리는 한 대칭적이라 비교가 깨지지 않는다.
크기 비교를 곁들이는 이유는 비용이 거의 들지 않으면서(메타데이터 조회 한 번에
mtime과 함께 나온다) 일부 도구가 mtime을 보존한 채 내용만 바꾸는 드문 경우까지
추가로 막아 주기 때문이다. snapshot 갱신을 저장 성공의 일부로 명시하는 이유는
첫 저장 후에도 이전 open 시점의 expected 값을 계속 쓰면 앱 자신의 저장을 다음
저장의 외부 충돌로 오인하기 때문이다.

## 구현 순서

각 단계가 독립적으로 검증 가능하도록 순서를 잡는다. 완료 정의는 저장소 전체와
동일하게 `cargo test` + `cargo check` + `pnpm build`다(`AGENTS.md:29`).

### 전제 — 반드시 code-pad 구현보다 먼저, 별도 PR로

기존 앱(everything-plus, knowledge-base)을 건드리는 리팩터이기 때문에 code-pad
구현과 한 PR에 섞으면 문제가 생겼을 때 원인이 "추출 자체의 실수"인지 "code-pad가
새 인터페이스를 잘못 썼는지" 분리할 수 없다. 두 PR을 먼저 merge한다.

0. **`crates/filesystem` 추출 PR** — everything-plus의 `core/indexer.rs`/
   `core/ignore.rs`에서 walk+ignore 로직만 이동(watcher는 이동하지 않음, "공통
   추출" 절 참고). `walk_files(root, max_entries)`와 `truncated` 반환을 정의하고,
   everything-plus는 무제한, code-pad는 50,000 제한으로 호출한다. everything-plus가 `path`
   의존으로 재연결. 검증:
   everything-plus의 기존 테스트(`indexer.rs`의 `collects_files_but_skips_ignored_dirs`,
   `text_ext_detection`, `ignore.rs`의 `ignores_common_dirs`, `keeps_normal_dirs`)가
   이동 후에도 그대로 통과해야 한다 + `cargo check --workspace`.
0. **`crates/markdown` 추출 PR** — knowledge-base의 `core/markdown.rs`를 이동,
   knowledge-base가 `path` 의존으로 재연결. generic crate에 없는
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
4. `core/guard.rs`: 임계치 경계 테스트(5MB, "확정된 세부 결정" 1번) → `cargo test`.
5. `core/session.rs`: JSON 왕복 + 손상 입력 테스트 → `cargo test`.
6. `commands/file.rs`: open/save 커맨드, `expectedMtime`+`expectedSize` 비교,
   성공 후 새 snapshot 반환, `crates/filesystem` 연결 → `cargo check`.
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
11. `QuickOpen.tsx`: `crates/filesystem`의 Rust-side `walk_files(..., 50_000)`
    목록과 `truncated` banner + 프론트 필터링 → vitest 필터 매칭.
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

## 의존성

- Rust: `encoding_rs = "0.8"`, `chardetng = "1"`, `notify = "8"`(everything-plus의
  `Cargo.toml:26`과 동일 계열 메이저 버전 — 이미 이 저장소 Cargo 워크스페이스에
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
