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
본업이다(`apps/everything-plus/src-tauri/Cargo.toml:5`, `description = "Everything+
Local Search"`). code-pad가 세 번째 파일 트리를 또 만드는 것은 이미 있는 두 개의
기능을 부분적으로 재구현하는 셈이 된다. 이 판단이 "확정된 UI 모델 — 폴더" 결정의
근거다.

### 목적

Notepad++를 대체할 가벼운 코드 에디터. 기반은 CodeMirror 6이다.

### 범위

**Phase 1**: 문법 하이라이팅, 탭, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크,
줄바꿈(CRLF/LF) 감지·변환, 인코딩 감지·변환, 큰 파일 가드, 단어 기반 자동완성,
프리뷰 패널(`.md`/`.mmd`).

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
상태 모양은 `views: [Doc[], Doc[]]` + `activeView` + 뷰별 활성 문서다. Notepad++
사용자가 옮겨올 때 그대로 아는 모델이라 학습 비용이 없고, 3개 이상으로 늘리는 임의
분할(VS Code류)보다 상태 전이 경우의 수가 훨씬 적어 "뷰 사이 문서 이동" 버그 표면을
좁힌다.

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
  `target` 등 20개 이름 매치)로 디렉터리를 걸러낸다. code-pad의 Ctrl+P가 같은 walk+
  ignore 조합을 두 번째로 실제 사용하므로, 지금이 "두 번째 소비자가 필요해진" 시점이다.
  두 함수를 그대로 옮길지, `IndexedFile`(크기·mtime을 담는 인덱싱 특화 구조체,
  `indexer.rs:6-11`)까지 옮길지 아니면 code-pad에 맞는 더 단순한 반환 타입으로
  분리할지는 이 문서의 범위가 아니다 — 추출 PR 자체가 그 설계를 정한다.
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
  code-pad가 프리뷰 패널에서 이 렌더러를 쓰는 지금이 그 "차례"다.
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
  lib.rs              run(), command 등록, watcher 기동
  commands/{file,folder,session}.rs
  core/               ← 전부 OS 비의존, cargo test 대상
    encoding.rs  line_ending.rs  guard.rs  session.rs
  watcher.rs          notify 기반 (IO — core 아님)

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
치명적이지 않은 이유는 아래 저장 흐름의 mtime 비교가 2차 방어선이기 때문이다.

### 흐름

- **열기**: `openFile(path)` → metadata(크기·mtime) 조회 → 임계치 초과면
  `read_only` 확정 → 바이트 읽기 → `encoding::detect()` → 디코딩 →
  `line_ending::detect()` → LF 정규화 → `OpenedFile { text, encoding, line_ending,
  read_only, mtime }` 반환.
- **저장**: `saveFile(path, text, encoding, lineEnding, expectedMtime)` → 디스크의
  현재 mtime이 `expectedMtime`과 다르면 `Err(Conflict)` → LF를 `lineEnding`으로
  변환 → `encoding`으로 인코딩 → 쓰기. 이 mtime 비교는 watcher가 놓친 레이스(함정 ②)에
  대한 2차 방어선이다 — watcher가 죽어 있어도(아래 에러 처리 표 참고) 이 비교 하나로
  덮어쓰기 사고를 막는다.
- **외부 변경**: watcher → 디바운스 → `app.emit("file-changed")` → 프론트가 그
  문서의 dirty 여부로 자동 리로드/배너를 분기한다(결정 4).
- **Ctrl+P**: 작업 폴더를 지정하면 `crates/filesystem`의 walk로 **1회** 목록을
  만들어 프론트 메모리에 보관하고, 타이핑에 따른 필터링은 전부 프론트에서 한다.
  목록이 상한(예: 50,000개)을 넘으면 안내만 하고 계속 쓴다(아래 "미결" 참고).
  watcher는 열려 있는 문서 전용이고 이 목록을 갱신하는 데 쓰지 않는다 — 폴더 전체를
  감시하는 비용을 지지 않는다.
- **세션**: 파일 열기·닫기·뷰 이동·활성 탭 변경마다 1초 디바운스로 `session.json`을
  저장한다.

## 상태 저장

`app_local_data_dir()`(CONVENTIONS §3 데이터 위치 규약, `CONVENTIONS.md:107-116`)
아래 **JSON 한 파일**에 세션과 설정(작업 폴더·최근 파일)을 담는다. 저장할 값이
세션 상태와 설정뿐이라 쿼리할 것이 없다.

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
| 큰 파일 | 읽기 전용 + 하이라이팅 비활성, 상태바에 사유 표시 |
| 인코딩 감지 실패 | UTF-8 lossy로 열고 표시 + 수동 인코딩 선택 제공 |
| 저장 시 인코딩 불가 문자 | **저장 중단** + UTF-8 전환 제안. 조용한 손실 금지(CP949 파일에 이모지를 넣는 실제 시나리오) |
| 저장 시 mtime 충돌 | 저장 중단 + 배너 |
| watcher 기동 실패 | 조용히 비활성화. mtime 검사가 폴백이라 치명적이지 않음 |
| `session.json` 손상 | 무시하고 빈 세션 시작 |
| 세션 속 파일 소실 | 그 항목만 건너뛰고 나머지 복원 |
| 프리뷰 렌더 실패 | knowledge-base와 동일 — 마지막 성공 유지 + 배지(근거는 knowledge-base 설계 문서의 결정 6, "마지막 성공 SVG 유지") |

## 테스트

code-pad는 이 저장소에서 **Rust core 테스트와 프론트 테스트가 처음으로 함께
작동하는 앱**이다. 지금까지는 Rust 쪽(`apps/`+`crates/` 전체 `#[test]` 85개,
2026-08-12 기준 `grep -rn '#\[test\]' apps crates | wc -l`로 확인)과 프론트 쪽이
따로 성숙해 왔다 — 프론트 테스트 인프라는 최근에야 저장소 전체에 깔렸다: 루트
`package.json`의 `devDependencies`에 `vitest`/`@testing-library/react`/`jsdom`이
추가되어 있고, 모든 앱의 `package.json`이 `"test": "vitest run --passWithNoTests"`를
갖고 있으며, `test(workspace): add vitest and cover pure logic + pane remount
(#46)` 커밋이 `apps/wsl-desktop/src/components/PaneCanvas.test.tsx`를 포함해
`.test.ts(x)` 파일 10개를 저장소 전역에 심었다. CI의 `frontend` 잡이 이미
`pnpm test`를 돌리므로(`.github/workflows/ci.yml:34-35`) code-pad가 vitest
스위트를 추가해도 새 CI 배선이 필요 없다.

- **Rust `cargo test`**: `encoding`(UTF-8/BOM/UTF-16LE·BE/CP949 감지, **왕복
  검증** — 디코딩→인코딩이 원본 바이트와 동일, CP949 표현 불가 문자 검출),
  `line_ending`(감지 + LF↔CRLF 왕복), `guard`(임계치 경계), `session`(JSON 왕복,
  손상 입력)
- **vitest**: 탭·뷰 상태 전이(뷰 간 이동, 마지막 탭 닫기, 활성 탭 이동), Ctrl+P
  필터 매칭
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

## 미결

이 브리핑에는 없었지만 구현 전에 정해야 하는 항목들이다. 바꾸지 않고 기록만 한다.

1. **큰 파일 가드의 정확한 임계치가 미정이다.** 브리핑은 "임계치"라고만 하고
   구체적인 바이트 수를 주지 않았다. 저장소 안의 다른 상한(everything-plus 콘텐츠
   인덱싱 1MB, `apps/everything-plus/src-tauri/src/core/indexer.rs:53`; 
   knowledge-base 이미지 인라인 2MB, `docs/superpowers/specs/2026-08-11-knowledge-base-markdown-preview-design.md:112`)은
   각각 다른 도메인(검색 인덱싱, 이미지 인라인)의 값이라 코드 에디터가 열어야 할
   "큰 텍스트 파일"(수십MB 로그 등) 감각에 그대로 재사용할 근거가 약하다.
2. **Ctrl+P 목록 상한이 "예: 50,000개"로 예시 수준으로만 제시됐다.** 정확한 값과
   초과 시 UX(부분 로드 여부, 안내 문구)가 정해지지 않았다.
3. **단어 기반 자동완성의 소스 범위가 미정이다.** 현재 문서만 스캔하는지, 열려
   있는 모든 문서(두 뷰 전체)를 스캔하는지, Ctrl+P가 만든 폴더 전체 인덱스까지
   확장하는지 브리핑에 없다. `@codemirror/autocomplete`의 기본 word-list 소스는
   단일 문서 기준이므로, 별도 결정 없이 기본값을 쓰면 그것이 곧 스코프가 되는데
   이게 의도된 범위인지 확인되지 않았다.
4. **북마크의 영속성 여부가 미정이다.** `editor/bookmarks.ts`가 세션 간 유지되는
   상태(`session.json`에 파일별 줄 번호로 기록)인지, 문서가 열려 있는 동안만
   유지되는 휘발성 상태인지 브리핑에 없다.
5. **수동 인코딩 선택 UI에 노출할 목록이 미정이다.** 브리핑은 감지 대상
   (UTF-8/BOM/UTF-16LE/BE/CP949)만 명시했다. 수동 선택 드롭다운이 이 5종과
   같은지, 더 넓은 목록(EUC-KR, Shift-JIS, ISO-8859-1 등)을 제공하는지 정해지지
   않았다.
6. **찾기/바꾸기(단일 파일 내)가 Phase 1 기능 목록에도 제외 목록에도 없다.**
   Notepad++ 대체가 목적인 앱에서 이례적인 공백이다. 새로 만들지 않고 공백으로만
   남긴다.
7. **`session.json`에 스키마 버전 필드가 있는지 미정이다.** "손상된 `session.json`은
   무시하고 빈 세션으로 시작"은 정해졌지만, 향후 필드 추가 시 마이그레이션을 위한
   `schemaVersion` 같은 필드를 처음부터 넣을지는 정해지지 않았다.
8. **`expectedMtime`의 직렬화 포맷과 비교 정밀도가 미정이다.** 유닉스 밀리초인지
   RFC3339 문자열인지, Windows NTFS의 mtime 해상도 안에서 안정적으로 비교되는지가
   브리핑에 없다.

## 구현 순서

각 단계가 독립적으로 검증 가능하도록 순서를 잡는다. 완료 정의는 저장소 전체와
동일하게 `cargo test` + `cargo check` + `pnpm build`다(`AGENTS.md:29`).

### 전제 — 반드시 code-pad 구현보다 먼저, 별도 PR로

기존 앱(everything-plus, knowledge-base)을 건드리는 리팩터이기 때문에 code-pad
구현과 한 PR에 섞으면 문제가 생겼을 때 원인이 "추출 자체의 실수"인지 "code-pad가
새 인터페이스를 잘못 썼는지" 분리할 수 없다. 두 PR을 먼저 merge한다.

0. **`crates/filesystem` 추출 PR** — everything-plus의 `core/indexer.rs`/
   `core/ignore.rs`에서 walk+ignore 로직만 이동(watcher는 이동하지 않음, "공통
   추출" 절 참고). everything-plus가 `path` 의존으로 재연결. 검증:
   everything-plus의 기존 테스트(`indexer.rs`의 `collects_files_but_skips_ignored_dirs`,
   `text_ext_detection`, `ignore.rs`의 `ignores_common_dirs`, `keeps_normal_dirs`)가
   이동 후에도 그대로 통과해야 한다 + `cargo check --workspace`.
0. **`crates/markdown` 추출 PR** — knowledge-base의 `core/markdown.rs`를 이동,
   knowledge-base가 `path` 의존으로 재연결. 검증: 기존 9개 테스트
   (`core/markdown.rs`의 `#[cfg(test)] mod tests`, 이 문서에서 읽어 확인한
   `frontmatter_is_stripped_before_render`부터 `non_mermaid_code_block_is_untouched`까지)가
   이동 후에도 그대로 통과해야 한다 + `cargo check --workspace`.

### code-pad 구현

1. 앱 스캐폴드: CONVENTIONS §6 절차(`CONVENTIONS.md:181-187`)로
   `pnpm create tauri-app` → 4곳 이름 교체 → 루트 `Cargo.toml`의 `members`에
   `apps/code-pad/src-tauri` 추가 → `tauri.conf.json` identifier
   `com.workbench.codepad`.
2. `core/encoding.rs`: 감지(UTF-8/BOM/UTF-16LE·BE/CP949) + 왕복 검증 테스트 →
   `cargo test`.
3. `core/line_ending.rs`: 감지 + LF↔CRLF 왕복 테스트 → `cargo test`.
4. `core/guard.rs`: 임계치 경계 테스트(미결 1의 값 확정 필요) → `cargo test`.
5. `core/session.rs`: JSON 왕복 + 손상 입력 테스트 → `cargo test`.
6. `commands/file.rs`: open/save 커맨드, mtime 비교, `crates/filesystem`·
   `crates/markdown` 연결 → `cargo check`.
7. `watcher.rs`: 부모 디렉터리 감시 + 파일명 필터링 + 디바운스(함정 ② 반영) →
   `cargo check`.
8. 프론트 `editor/`: CodeMirror 6 배선(`CodeEditor.tsx`, `extensions.ts`),
   `@codemirror/lang-*` 언어팩으로 문법 하이라이팅 → `pnpm build`.
9. 프론트 탭·뷰 상태(`views`/`activeView`) + `ViewPane.tsx`: `PaneCanvas.tsx`의
   "하나의 안정된 부모 + CSS `display`/`order`" 패턴을 그대로 적용(아래 "뷰 사이
   문서 이동" 참고) → vitest(뷰 간 이동/마지막 탭 닫기/활성 탭 이동) + 컴포넌트
   테스트(`PaneCanvas.test.tsx` 패턴 재사용).
10. `QuickOpen.tsx`: `crates/filesystem` walk 1회 목록 + 프론트 필터링 →
    vitest 필터 매칭.
11. `PreviewPane.tsx`: `crates/markdown` 연결(`.md`), `.mmd` 단독 파일 렌더
    분기 → `pnpm build`.
12. `StatusBar.tsx`: 인코딩/줄바꿈/읽기전용 표시 배선.
13. 멀티커서/사각선택/북마크(`editor/bookmarks.ts`)/단어 기반 자동완성
    (`@codemirror/autocomplete`) CodeMirror 확장 배선 → `pnpm build`.
14. Windows에서 `pnpm tauri dev`로 수동 검증: 실제 CP949 파일, CRLF 파일, 큰
    파일, git 브랜치 전환 중 리로드(WSL에서는 `pnpm tauri dev`를 돌릴 수 없으므로
    이 단계만 Windows 필요, `CONVENTIONS.md §1`).

### 뷰 사이 문서 이동 — wsl-desktop 교훈 적용 (9단계 상세)

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

채택된 해법은 **모든 문서를 하나의 부모 아래 안정된 배열 순서로 두고, 표시 여부는
CSS `display: none`, 순서는 CSS `order`로만 제어**하는 것이다. 배열 자체는 절대
재정렬하지 않는다. 실제 구현:

```tsx
// PaneCanvas.tsx:58-88 요지 — code-pad의 ViewPane도 이 구조를 그대로 쓴다
<div className="panes" style={gridStyle}>
  {panes.map((pane) => {
    const order = activePaneIds.indexOf(pane.id);
    const active = order !== -1;
    return (
      <TermPane
        key={pane.id}
        /* ... */
        style={active ? { order } : { display: "none" }}
      />
    );
  })}
</div>
```

`panes` 배열은 추가는 `[...prev, pane]`(append만), 제거는 `filter`만 써서 상대
순서를 보존한다. `.panes`의 자식 목록이 항상 "지금까지 만들어진 모든 문서, 생성
순서 그대로"이고 뷰 전환·문서 이동으로 바뀌는 것이 각 자식의 `style`뿐이라면,
React는 매번 같은 부모의 같은 위치에서 같은 key를 발견하므로 재조정만 하고 fiber를
옮길 일 자체가 없다.

`PaneCanvas.test.tsx`가 이 불변을 검증하는 방식도 그대로 재사용한다 — 자식 컴포넌트를
모킹해 마운트/언마운트를 스파이로 잡고(`PaneCanvas.test.tsx:13-36`), 전환 전후로
`unmountSpy`가 호출되지 않았음을 확인한다(`PaneCanvas.test.tsx:84-98`,
"탭을 전환해도 비활성화된 팬은 언마운트되지 않고, 새로 마운트되지도 않는다").
code-pad에서는 CodeMirror 인스턴스를 감싸는 컴포넌트를 모킹해 같은 스파이 패턴을
적용한다.

## 의존성

- Rust: `encoding_rs = "0.8"`, `chardetng = "1"`, `notify = "8"`(everything-plus의
  `Cargo.toml:26`과 동일 계열 메이저 버전 — 이미 이 저장소 Cargo 워크스페이스에
  8.x가 들어와 있으므로 맞춰 둔다), `serde`/`serde_json`.
- 프론트: `codemirror` 6 + `@codemirror/lang-*` 언어팩, `@codemirror/autocomplete`,
  `mermaid`(프리뷰 — knowledge-base가 이미 `^11.16.1`을 쓰고 있다,
  `apps/knowledge-base/package.json:15`. pnpm workspace에서 동일 계열 버전을
  맞추면 중복 설치를 줄일 수 있지만, 정확한 버전 고정은 code-pad 구현 시점에
  최신 안정판을 확인해 정한다).

`encoding_rs`/`chardetng`은 저장소 어떤 `Cargo.toml`에도 아직 없다(확인:
`grep -rln "encoding_rs\|chardetng" apps/*/src-tauri/Cargo.toml Cargo.toml` 결과
없음) — code-pad가 첫 도입이다.
