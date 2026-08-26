# code-pad — Code Pad (경량 코드 에디터)

Notepad++를 대체할 가벼운 코드 에디터. CodeMirror 6 기반, 언어 중립 LSP 클라이언트를 내장한다.
산출물: `Code Pad.exe` (`apps/code-pad`).

## 주요 기능

- **편집** — 문법 하이라이팅, 탭·분할 2뷰, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크. 탭과 CodeMirror 본문은 우클릭·Shift+F10·Menu 키 컨텍스트 메뉴를 제공
- **탭 파일 작업** — 닫기·다른/오른쪽 탭 닫기, canonical 경로 복사·탐색기 표시, 파일 이름 변경·삭제. 여러 dirty 탭은 순차 확인하고 이름 변경·삭제는 디스크 스냅샷을 다시 검증
- **인코딩/줄바꿈** — 인코딩 감지·변환, CRLF/LF 감지·변환, 큰 파일 가드
- **찾기/바꾸기** — 단일 파일 내 정규식 지원
- **빠른 열기** — 작업 폴더를 한 번만 제한 색인한 뒤 파일명·상대 경로를 fuzzy 검색하고, 디렉터리 트리로 묶어 긴 경로도 이름과 부모 경로로 나누어 표시. 마우스 없이 `Ctrl/⌘+P` → 입력 → `↑/↓`·`Home/End` → `Enter`로 파일을 연다
- **프리뷰** — `.md`/`.mmd`(mermaid) 프리뷰 패널. 편집기와 프리뷰는 서로 다른 배경과
  경계로 구분되며, 편집기 본문에는 활성 경계가, 탭에는 키보드 포커스 링이 표시된다.
  긴 경로와 본문은 줄바꿈하고, 코드 블록은 패널 안에서 가로 스크롤하며, 미디어와
  다이어그램은 패널 너비를 넘지 않는다. 프리뷰 본문만 세로 스크롤을 소유한다. 이 시각
  구분 작업에서는 프리뷰 renderer·상태·IPC·원문/path 전달 동작을 변경하지 않는다.
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

## 기술

- CodeMirror 6 (공용 `packages/editor`), `packages/diff-view`
- 공용 크레이트 `crates/filesystem`·`crates/markdown`
- mermaid `securityLevel: "strict"`
- LSP 관리 dialog는 header/footer를 고정하고 본문 하나만 scroll한다. 상태와 설치 목록에 별도 nested
  scroll을 만들지 않으며 viewport 안에서 최대 900px 높이를 사용한다
- 파일 이름 변경은 같은 폴더의 단일 이름만 허용하고 기존 대상을 덮어쓰지 않는다. 삭제는 복구 불가·미저장 버퍼 손실을 명시적으로 확인하며, 두 작업 모두 mtime·크기·SHA-256이 열린 탭의 스냅샷과 일치할 때만 실행한다
- clipboard IPC는 `allow-read-text`만 허용한다. 사용자가 편집기 메뉴의 붙여넣기를 고른 순간에만 plain text를 읽어 현재 CodeMirror selection에 삽입하며 history·background 수집은 하지 않는다

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-code-pad-design.md`
