# code-pad — Code Pad (경량 코드 에디터)

Notepad++를 대체할 가벼운 코드 에디터. CodeMirror 6 기반, 언어 중립 LSP 클라이언트를 내장한다.
산출물: `Code Pad.exe` (`apps/code-pad`).

## 주요 기능

- **편집** — 문법 하이라이팅, 탭·분할 2뷰, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크. 탭과 CodeMirror 본문은 우클릭·Shift+F10·Menu 키 컨텍스트 메뉴를 제공
- **탭 파일 작업** — 닫기·다른/오른쪽 탭 닫기, canonical 경로 복사·탐색기 표시, 파일 이름 변경·삭제. 여러 dirty 탭은 순차 확인하고 이름 변경·삭제는 디스크 스냅샷을 다시 검증
- **인코딩/줄바꿈** — 인코딩 감지·변환, CRLF/LF 감지·변환, 큰 파일 가드
- **찾기/바꾸기** — 단일 파일 내 정규식 지원
- **프리뷰** — `.md`/`.mmd`(mermaid) 프리뷰 패널
- **LSP** — Windows 로컬 stdio 서버 관리(진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프).
  rust-analyzer·typescript-language-server·basedpyright·vscode-langservers-extracted를 고정 버전으로 설치

## 기술

- CodeMirror 6 (공용 `packages/editor`), `packages/diff-view`
- 공용 크레이트 `crates/filesystem`·`crates/markdown`
- mermaid `securityLevel: "strict"`
- 파일 이름 변경은 같은 폴더의 단일 이름만 허용하고 기존 대상을 덮어쓰지 않는다. 삭제는 복구 불가·미저장 버퍼 손실을 명시적으로 확인하며, 두 작업 모두 mtime·크기·SHA-256이 열린 탭의 스냅샷과 일치할 때만 실행한다
- clipboard IPC는 `allow-read-text`만 허용한다. 사용자가 편집기 메뉴의 붙여넣기를 고른 순간에만 plain text를 읽어 현재 CodeMirror selection에 삽입하며 history·background 수집은 하지 않는다

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-code-pad-design.md`
