# code-pad — Code Pad (경량 코드 에디터)

Notepad++를 대체할 가벼운 코드 에디터. CodeMirror 6 기반, 언어 중립 LSP 클라이언트를 내장한다.
산출물: `Code Pad.exe` (`apps/code-pad`).

## 주요 기능

- **편집** — 문법 하이라이팅, 탭·분할 2뷰, 확대/축소, 멀티커서, 사각(영역) 선택, 북마크
- **인코딩/줄바꿈** — 인코딩 감지·변환, CRLF/LF 감지·변환, 큰 파일 가드
- **찾기/바꾸기** — 단일 파일 내 정규식 지원
- **프리뷰** — `.md`/`.mmd`(mermaid) 프리뷰 패널
- **LSP** — Windows 로컬 stdio 서버 관리(진단·자동완성·hover·정의·참조·이름 변경·포맷, 재시작 백오프).
  rust-analyzer·typescript-language-server·basedpyright·vscode-langservers-extracted를 고정 버전으로 설치

## 기술

- CodeMirror 6 (공용 `packages/editor`), `packages/diff-view`
- 공용 크레이트 `crates/filesystem`·`crates/markdown`
- mermaid `securityLevel: "strict"`

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-12-code-pad-design.md`
