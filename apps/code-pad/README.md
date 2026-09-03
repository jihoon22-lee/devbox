# code-pad — Code Pad (경량 코드 에디터)

Notepad++를 대체할 가벼운 코드 에디터. CodeMirror 6 기반, 언어 중립 LSP 클라이언트를 내장한다.
산출물: `Code Pad.exe` (`apps/code-pad`).

## 주요 기능

- **편집** — 문법 하이라이팅, 탭·분할 2뷰, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크. 탭과 CodeMirror 본문은 우클릭·Shift+F10·Menu 키 컨텍스트 메뉴를 제공
- **탭 파일 작업** — 닫기·다른/오른쪽 탭 닫기, canonical 경로 복사·탐색기 표시, 파일 이름 변경·삭제. 여러 dirty 탭은 순차 확인하고 이름 변경·삭제는 디스크 스냅샷을 다시 검증
- **인코딩/줄바꿈** — 인코딩 감지·변환, CRLF/LF 감지·변환, 큰 파일 가드
- **찾기/바꾸기** — 단일 파일 내 정규식 지원
- **빠른 열기** — 작업 폴더를 한 번만 제한 색인한 뒤 파일명·상대 경로를 fuzzy 검색하고, 디렉터리 트리로 묶어 긴 경로도 이름과 부모 경로로 나누어 표시. 읽기 실패가 있으면 불완전 목록임을 숨기지 않는다. 마우스 없이 `Ctrl/⌘+P` → 입력 → `↑/↓`·`Home/End` → `Enter`로 파일을 연다
- **프리뷰** — `.md`/`.mmd`(mermaid) 프리뷰 패널. 편집기와 프리뷰는 서로 다른 배경과
  경계로 구분되며, 편집기 본문에는 활성 경계가, 탭에는 키보드 포커스 링이 표시된다.
  긴 경로와 본문은 줄바꿈하고, 코드 블록은 패널 안에서 가로 스크롤하며, 미디어와
  다이어그램은 패널 너비를 넘지 않는다. 프리뷰 본문만 세로 스크롤을 소유한다. Mermaid
  runtime은 다이어그램이 실제로 나타나는 첫 프리뷰에서만 공용 renderer를 동적 로드하며,
  일반 Markdown의 초기 editor bundle에는 포함하지 않는다.
- **LSP** — Windows 로컬 stdio 서버 관리(진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프).
  상태와 retry/circuit, 검증된 관리형 runtime cache를 한 화면에서 확인하고 `다시 시도`로 명시적 복구한다.
  최근 로그는 앱 실행 중 memory에 최대 64개 언어·언어별 200개만 보존하며, 제3자 서버 stderr는 native 경계에서
  절대 경로·URL·credential 패턴을 제거하고 길이를 제한한 뒤 표시한다. raw stderr·설정 오류는 IPC로
  전달하지 않는다. rust-analyzer·typescript-language-server·basedpyright·vscode-langservers-extracted를
  검토된 고정 버전으로 명시적으로 설치할 수 있다. 설치가 성공하면 archive는 app-owned
  `lsp/downloads/cache/<sha256>.<ext>`에 크기·SHA-256 검증 후 보관하고, 다음 설치에서 network 없이
  재사용한다. native 서버는 같은 검증을 통과한 local archive를 가져올 수 있고, Node 서버는 native
  multi-file picker로 reviewed dependency closure의 `.tgz`들을 선택해 exact size·SHA-256·lock
  integrity를 확인한다. 선택 set은 이미 검증된 cache와 결합해 설치할 수 있으며, 선택한 원본 경로는
  index·status·로그에 저장하거나 반환하지 않는다.
  LSP 이름 변경은 요청과 적용을 분리한다. 서버가 반환한 text-only WorkspaceEdit를 작업 폴더
  내부의 상대 경로·범위·before/after diff로 먼저 미리 보여주고, 사용자가 `전체 적용`을 승인한
  뒤에만 저장한다. 적용 직전 각 파일의 mtime·크기·SHA-256을 다시 확인하며, 열린 dirty 문서,
  손실 디코딩·읽기 전용 파일, 작업 폴더 밖 URI, resource operation은 거부한다. 여러 파일 중
  하나라도 쓰기 또는 LSP 반영에 실패하면 app-local `0700` transaction 디렉터리의
  identity/hash 검증 백업으로 이미 쓴 파일을 역순 되돌리고 파일별 결과를 표시한다.
  취소·timeout과 서버 중단 시에도 pending plan을 폐기하며, journal recovery는 외부 변경
  파일을 덮어쓰지 않는다. 미리보기의 before/after와 range payload는 각각 UTF-8
  16KiB·전체 저장 결과는 정규화/인코딩 후 2MiB aggregate 안에서만 허용하며, UTF-16·CRLF
  확장으로 이 상한을 넘으면 적용 전에 거부한다. 미리보기에는 절대 경로·서버 오류·credential이
  포함되지 않으며, 서버가 반환한 전체 텍스트는 native pending plan에만 보관한다. Windows
  drive/UNC 및 W3 long path는 canonical component 기준으로 상대 경로를 만들고, 적용·rollback
  경계에서 final reparse point/symlink를 fail-closed로 거부한다. 적용 중 Ctrl/⌘+Escape와
  `취소`가 먼저 도착해도 mirror flush가 끝난 뒤 native apply를 호출하지 않는다.
- **WSL 작업 폴더** — `\\wsl$`, `\\wsl.localhost`, canonical `\\?\UNC\wsl.localhost`
  경로의 Linux 부분은 대소문자를 보존한다. 파일 읽기·편집·원자 저장은 지원하고 열린 파일의
  외부 변경은 5초 bounded polling으로 감지한다. Windows host LSP는 지원하지 않으며, 편집 실패로
  오인되지 않도록 workspace 상태와 LSP 설정 화면에 같은 고정 사유를 표시한다. live file watcher는
  512개로 제한해 모든 등록 파일을 한 poll cycle에서 확인하며, 초과 등록은 조용히 누락하지 않고
  명시적으로 거부한다.

## 기술

- CodeMirror 6 (공용 `packages/editor`), `packages/diff-view`
- 공용 크레이트 `crates/filesystem`·`crates/markdown`·`crates/wsl`
- 공용 `packages/mermaid-renderer`의 lazy Mermaid, `securityLevel: "strict"`
- LSP 관리 dialog는 header/footer를 고정하고 본문 하나만 scroll한다. 상태와 설치 목록에 별도 nested
  scroll을 만들지 않으며 viewport 안에서 최대 900px 높이를 사용한다
- LSP diagnostics와 status frontend listener는 등록·실패·해제를 서로 독립적으로 관리한다. 한쪽
  등록만 실패하거나 effect 정리 뒤 등록이 완료돼도 성공한 listener를 잃지 않고 정확히 해제한다.
- 파일 이름 변경(탭 컨텍스트 메뉴)은 같은 폴더의 단일 이름만 허용하고 기존 대상을 덮어쓰지 않는다. 삭제는 복구 불가·미저장 버퍼 손실을 명시적으로 확인하며, 두 작업 모두 mtime·크기·SHA-256이 열린 탭의 스냅샷과 일치할 때만 실행한다. LSP의 여러 파일 내용 변경은 위의 별도 preview/rollback 경로를 사용한다.
- clipboard IPC는 `allow-read-text`만 허용한다. 사용자가 편집기 메뉴의 붙여넣기를 고른 순간에만 plain text를 읽어 현재 CodeMirror selection에 삽입하며 history·background 수집은 하지 않는다

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-code-pad-design.md`
