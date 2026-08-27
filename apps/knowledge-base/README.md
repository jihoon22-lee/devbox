# knowledge-base — 개인 지식 저장소

Markdown-first로 설계한 개인 지식·프로젝트·일일 기록 관리 앱. 파일 자체가 데이터의 원본이 되어 앱이 없어도 내용을 읽을 수 있다.
산출물: `Knowledge.exe` (`apps/knowledge-base`).

## 주요 기능

- **폴더/파일 탐색** — 왼쪽 트리, 생성/이름변경/이동/삭제. 우클릭·Shift+F10·Menu 키 메뉴에서 경로 복사, 탐색기 표시, 설치된 catalog 대상 앱으로 열기를 지원
- **Markdown 편집** — CodeMirror(공용 `packages/editor`) + 프리뷰 토글, Ctrl+S 저장, mermaid 다이어그램 렌더. CM6 DOM event 경유 메뉴에서 잘라내기·복사·명시적 붙여넣기·Markdown 링크 삽입을 지원
- **이미지 붙여넣기·드롭** — Markdown 노트 편집 중 클립보드 이미지 붙여넣기와 단일 이미지 드롭을
  지원한다. PNG/JPEG/GIF/WebP만 native에서 판정하고, vault 루트 `assets/`에 content hash 이름으로
  저장한 뒤 현재 노트 기준 relative Markdown image node를 편집기에 삽입한다. 같은 bytes는 재사용하며
  원래 파일명은 보존하지 않는다
- **Wikilink / backlink** — `[[target]]`·`[[target|alias]]` 자동완성, resolved/unresolved 표시,
  Ctrl/Cmd+클릭 노트 이동과 backlink source line·column 이동. fenced/inline code와 escape된 문법은
  링크로 인덱스하지 않는다
- **안전한 이름 변경** — 파일·폴더의 새 경로와 깨질 위키링크만 diff로 먼저 표시하고 전체
  승인을 받은 뒤 하나의 one-shot transaction으로 적용. 별칭은 보존하고 title 등으로 계속
  유일하게 해석되는 링크는 불필요하게 다시 쓰지 않는다
- **검색** — 제목+본문 FTS5 (`crates/search`)
- **태그** — YAML frontmatter(`tags:`) 파싱, 태그 목록·필터
- **데일리 노트** — 날짜별 생성·연결
- **빠른 캡처** — `Ctrl+Alt+K`(Windows 전역 단축키) 또는 앱 내 버튼으로 제목·본문·태그를
  먼저 미리 본 뒤 오프라인 `Inbox/`에 새 Markdown 노트를 저장한다. 미리보기는 native가
  발급한 일회성 opaque approval으로 저장하며, 취소·닫기·stale 응답은 실제로 폐기된다.
  단축키 충돌이어도 앱 내 동작은 유지하며, 클립보드는 사용자가 선택한 순간에만 한 번 읽는다
- **앱 간 열기** — catalog의 `Path`로 Knowledge root 안의 Markdown 노트를 열고, `Query`로 즉시 검색. cold start와 실행 중 재호출 모두 같은 pending-open 경로를 사용
- **Life Log draft 받기** — `knowledge-draft/v1` handoff를 claim한 뒤 저장 전 요약/출처/태그/본문을 preview한다. 사용자가 승인한 경우에만 새 Journal note를 만들고 handoff를 소비한다
- **활동 snapshot** — 오늘 작성·수정된 노트 수와 경로 없는 불투명 식별자를 Life Log용 `activity/v1` view로 발행

## 기술

- 파일을 원본(source of truth)으로 두고 SQLite는 검색용 보조 인덱스
- SQLite의 `doc_link_keys`·`wikilinks`도 재생성 가능한 보조 인덱스다. path stem·filename·title이
  정확히 한 노트에만 대응할 때 resolved로 판정하며 중복 title/filename은 ambiguous unresolved로
  처리한다. 새 노트가 생기거나 watcher가 외부 편집을 반영하면 source를 다시 쓰지 않아도 현재
  key 집합 기준으로 resolution과 backlink가 갱신된다
- wikilink schema 최초 실행에는 root의 안전한 Markdown 원문을 한 번 읽어 source position을
  복구한다. root 밖 symlink·비 Markdown·10 MiB 초과·읽기 실패 항목은 인덱스에 넣지 않고,
  일반 링크 DTO에는 절대 경로와 본문을 포함하지 않는다
- `crates/markdown` `sanitize()`로 HTML 살균, mermaid `securityLevel: "strict"`
- `core/store.rs`의 자체 `safe_join`으로 루트 밖 경로 차단
- 트리 메뉴의 filesystem·launch 명령은 실행 직전에 항목과 기존 조상을 canonicalize하고 symlink 경유 루트 탈출을 거부한다. absolute path는 사용자가 경로 복사를 선택한 경우에만 frontend에 반환하며, 다른 앱으로 열기는 catalog capability와 실제 설치 상태를 다시 검증한다
- 이름 변경 미리보기는 canonical root 안에서 파일·폴더 목적지를 다시 검증하고 root 경로 목록,
  모든 Markdown 원문, 이동 대상 내부 파일의 SHA-256 스냅샷을 만든다. root 10,000항목·스냅샷
  64 MiB·rewrite 200파일/5,000링크 상한을 넘으면 변경 전에 중단한다. plan 원문은
  `Serialize`/`Debug`하지 않는 app-managed slot 한 개에만 보관하고 opaque ID로 한 번만 적용하거나
  명시적으로 폐기한다. 파일 rename은 link index의 파일 종류가 preview와 달라지지 않도록 Markdown
  여부(`.md`/비 Markdown)를 유지해야 한다
- 적용 직전 같은 스냅샷을 다시 계산하고 destination·경로 종류·Knowledge root가 달라졌으면 전체를
  중단한다. 통과하면 링크 파일별 atomic replace, source rename, SQLite FTS/link transaction을
  수행한다. 파일 또는 DB 단계가 실패하면 이미 쓴 링크와 rename, 새로 만든 빈 parent directory를
  되돌린다. 이는 여러 파일을 한 OS primitive로 바꾸는 전역 원자성이 아니라 bounded preflight와
  파일별 atomic replace 및 rollback으로 제공하는 실행 중 오류의 all-or-rollback 계약이다. 프로세스나
  OS가 apply 도중 강제 종료되는 경우를 복구하는 영속 journal은 이 범위에 포함하지 않는다
- 폴더 삭제 시 하위 FTS/link row를 함께 제거한다. 이름 변경은 이동 대상의 읽을 수 있는 파일과
  외부에서 rewrite된 Markdown을 새 상대 경로·내용으로 같은 SQLite transaction에 재색인한다
- 자동완성은 root-relative path에서 `.md`를 뺀 canonical target을 삽입한다. raw target을 파일
  경로로 직접 열지 않으며, editor/preview/backlink 이동은 backend가 유일하게 resolve한 상대
  경로를 기존 canonical root·`.md`·10 MiB 검증 경계에서 다시 연다
- inbound Path는 canonical Knowledge root 내부의 실제 `.md` 파일만 허용하고 10 MiB로 제한한다. 실패 시 raw path·OS 오류를 UI에 반향하지 않는다
- `crates/integration`의 multi-view envelope을 사용해 `%LOCALAPPDATA%\devbox\integration\knowledge-base\v1\summary.json`을 원자 교체한다
- `activity/v1` entry는 `notesModifiedToday`, `lastModifiedAtMs`, `noteIds`, `identifiersTruncated`만 포함한다. `noteIds`는 DB row에서 만든 `note-<양의 정수>` 형식이며 최대 512개다
- 노트 경로·제목·본문·tag·credential은 snapshot에 포함하지 않는다. 앱 저장·생성·이름변경·삭제·데일리 노트 생성과 watcher가 감지한 외부 편집 뒤에 같은 snapshot을 best-effort로 갱신한다
- Life Log `knowledge-draft/v1` 수신은 `sourceApp=life-log`, `targetApp=knowledge-base`, exact kind/schema,
  fixed source order, aggregate-only summary, bounded body/tags/title와 deterministic Markdown body를
  모두 검증한다. 공용 handoff claim token은 process-local slot에만 보관하고 frontend에는 opaque id,
  token, filesystem path를 노출하지 않는다. preview는 파일을 만들지 않으며, 명시적 `Save draft`가
  exclusive `Journal/YYYY-MM-DD-life-log-<period>.md` create와 SQLite search index 갱신을 완료한
  뒤 ack/delete한다. 같은 날짜 파일은 suffix를 붙여 보존하고 덮어쓰지 않는다. cancel 또는
  validation/file/index 실패는 claim을 restore하고, 만료·손상·잘못된 target은 고정 안내만 표시한다.
  lease는 30초 주기로 갱신하되 10분 envelope TTL은 연장하지 않는다. Life Log DB를 직접 읽거나
  네트워크/LLM을 호출하지 않으며, browser preview에서는 native handoff API를 지원하지 않는다
- handoff preview/save는 이미 설정된 absolute vault만 읽고 `Documents/Knowledge` 기본값이나
  `Journal`을 수신 부수효과로 만들지 않는다. preview가 캡처한 `VaultIdentity`(canonical root와
  filesystem identity)는 save 직전과 publication 전후에 재검증하며, root 교체·symlink/reparse
  component·Journal 종류 변경은 새 digest를 요구하는 고정 오류로 중단한다. 완전히 flush한 임시
  파일은 no-replace publication으로만 Journal에 연결하고, SQLite index transaction이 실패하면
  같은 entry identity일 때만 파일을 정리한 뒤 claim을 restore한다.
- 외부 편집 watcher는 bounded event queue와 이벤트당 128개·4KiB path, 4,096개 경로 debounce
  상한을 사용하고, 현재 identity 안의 regular UTF-8 문서만 최대 10 MiB까지 읽는다. modal은 title/body UTF-8 byte
  사용량, initial focus·Escape·Tab trap·focus restore를 제공하며, stale/expiry/unmount 응답과
  중복 Save/Cancel은 화면 또는 native 상태를 다시 오염시키지 않는다
- clipboard IPC는 `allow-read-text`만 허용하며 편집기에서 사용자가 붙여넣기를 고른 순간의 plain text만 읽는다. clipboard history나 background 수집은 하지 않는다
- quick capture도 같은 clipboard read 권한을 사용하지만, 별도의 history·자동 수집은 하지 않는다.
  미리보기와 저장 모두 Rust가 최종 입력·태그·본문 상한, 제어문자, credential-like 패턴과
  고정 `Inbox` 경계를 다시 검사한다. 민감한 입력이나 native 저장 실패는 고정된 안전 메시지만
  반환하며 입력·절대 경로·OS 오류를 UI/로그에 반향하지 않는다. 브라우저 미리보기도 같은
  Unicode scalar/UTF-8 byte·line separator·credential policy를 먼저 적용하며, clipboard 값은
  이 검사를 통과한 경우에만 controlled draft에 넣는다
- quick capture native 저장은 `Inbox/quick-capture-YYYY-MM-DD-HH-mm-ss[-N].md` 형식의
  UTC 파일명을 사용한다. 같은 디렉터리의 bounded temporary sibling을 `create_new`로
  flush/sync한 뒤 Unix hard-link 또는 Windows no-replace `MoveFileExW`로 publish하므로
  경쟁 파일을 덮어쓰지 않는다(Unix parent directory `fsync`, Windows
  `MOVEFILE_WRITE_THROUGH`). vault identity가 유지되는 동안 SQLite FTS/link index
  transaction이 실패하면 새 파일과 temporary residue를 정리해 반쪽 노트를 남기지 않는다.
  identity가 바뀐 경우에는 경로를 따라 삭제하지 않고 stale로 중단해 교체된 vault를 보호한다.
  본문 line ending은 LF로 정규화하고
  동일 입력의 Markdown은 결정론적으로 생성한다. 정규화된 본문은 64 KiB, raw CRLF 입력은
  128 KiB, 제목은 200 scalar/800 byte, 태그는 20개·항목 48 scalar/192 byte·총 1 KiB로
  제한하며 renderer도 같은 경계를 다시 확인한다
- preview는 이미 설정된 root를 읽어 고정 `Inbox`를 metadata로만 확인하고 폴더나 기본 root를
  초기화하지 않는다. preview는 vault canonical path와 filesystem identity를 기억하고 save
  직전에 root·existing ancestor·`Inbox`를 반복 재검사한다. save만 검증된 고정 한 단계
  디렉터리를 지연 생성하며, root 또는 `Inbox`가 교체·파일·symlink·Windows reparse point로
  바뀌면 approval을 stale 처리하고 쓰기를 중단한다. 반환되는 저장 path도 고정된 timestamp
  filename grammar만 허용하며 임의의 상대·절대 path를 UI 계약으로 받아들이지 않는다
- `Ctrl+Alt+K` 등록은 Windows `RegisterHotKey` 메시지 루프가 담당한다. 다른 앱이 이미
  사용 중이거나 플랫폼이 지원되지 않으면 상태를 `conflict`/`unsupported`로만 표시하고,
  앱 내부 빠른 캡처 버튼은 계속 사용할 수 있다. worker는 실제 message queue를 만든 뒤
  종료 시 `WM_QUIT`를 받아 unregister하고 join하며, 중복 listener·늦은 hotkey event를
  무시한다. 단축키 event에는 문서 내용·경로가 없다
- watcher는 bounded sync channel·debounce map·event/path/file/reconcile 상한을 사용한다.
  overflow는 제한된 root reconcile로 수렴하며 symlink/reparse와 10 MiB 초과 파일은 읽지
  않는다. clipboard/serde 입력도 raw body·tag list·preview ID·image base64 envelope를
  native와 UI 양쪽에서 bounded 처리해 큰 payload가 modal state나 app-managed slot에 머물지
  않게 한다
- #303 acceptance는 quick capture와 Inbox note 흐름으로 독립 유지하며, 이 PR에서 함께 묶인 #304
  image asset acceptance는 아래 image 항목과 별도 fixture/workthrough로 검증한다. template, cloud
  sync, clipboard history 및 다른 앱으로의 handoff는 양쪽 범위에서 구현하지 않는다
- 이미지 입력도 백그라운드 clipboard 수집이나 clipboard history를 만들지 않는다. Ctrl/Cmd+V의
  browser paste/drop event 또는 사용자가 편집기 context menu에서 명시적으로 고른 순간의
  `navigator.clipboard.read()`만 사용한다. 지원되지 않는 WebView의 이미지 Clipboard API는
  plain-text paste로 안전하게 fallback하며, 자동 업로드·외부 image hosting은 하지 않는다
- 이미지 자산 저장 경계는 frontend와 native 양쪽에 있다. bytes는 2 MiB 이하로 먼저 제한하고,
  native `save_image_asset`이 magic 및 dimension을 최종 판정한다. PNG/JPEG/GIF/WebP의 최대 한
  변은 16,384px, 총 pixel은 64M이며 MIME과 원본 filename은 신뢰하지 않는다. 저장 파일은
  `assets/<sha256 lowercase>.<png|jpg|gif|webp>`로만 생성된다
- `assets/`는 vault 바로 아래의 고정 디렉터리이며 symlink/reparse 또는 파일로 바뀐 경우 전체
  작업을 거부한다. 생성 시 bounded temp file을 write/flush/sync한 뒤 no-overwrite atomic
  publication(Unix hard-link + parent directory `fsync`, Windows non-replacing
  `MoveFileExW(..., MOVEFILE_WRITE_THROUGH)`)을 하고, 동일 hash의 동일 bytes만
  `reused`로 재사용한다. 충돌·partial write·경로
  오류는 고정된 안전 오류로 반환하고 기존 파일을 덮어쓰지 않는다
- Markdown 링크는 노트 위치에서 `../`를 계산한 POSIX relative destination으로 native가 생성한다.
  nested note의 `../../assets/...`도 preview에서 vault 내부로만 normalize하며 absolute path,
  drive/UNC path, traversal·control 문자·외부 URI를 거부한다. asset command는 문서 파일을
  직접 수정하지 않고, 성공한 node만 현재 CodeMirror 초안에 삽입하며 실제 노트 파일 변경은
  사용자가 명시적으로 Save를 선택할 때만 일어난다
- image paste/drop은 한 번에 한 파일만 처리한다. 처리 중인 두 번째 action, IME paste,
  unmounted editor, 다른 note로 전환된 stale 응답, 변경된 문서에 대한 늦은 응답은 저장/삽입하지
  않는다. ordinary text paste와 IME 경로는 기존 CodeMirror 동작을 유지한다. OCR, asset
  transformation, clipboard history, external hosting, cloud sync는 이 기능에 포함하지 않는다
- 이름 변경은 외부 binary, network, runtime download 없이 동작한다. 직접 추가한 `sha2 0.11`은
  기존 workspace lock과 고지에 있던 MIT/Apache-2.0 dependency이며, preview UI는 기존
  `@devbox/diff-view`를 세 번째 소비자로 재사용한다

## 데이터

- 노트 파일: `Documents\Knowledge`
- 검색 인덱스: `%LOCALAPPDATA%\com.devbox.knowledgebase\data.db`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
