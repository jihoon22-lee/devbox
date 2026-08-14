# @devbox/editor

code-pad·knowledge-base가 공유하는 CodeMirror 6 공용 설정 프리미티브.

- `baseEditorExtensions({ searchPanel })` — 라인번호·히스토리·활성 라인·기본 keymap·검색
- `baseKeymap()` — 기본 + 히스토리 + 검색 keymap
- `languageForPath` / `languageExtensionFor` — 확장자 기반 언어 판정
- `markdownEditorExtensions()` — knowledge-base용 마크다운 확장
- `syntaxHighlightingExtension` / `readOnlyExtension`

테마·전체 state·LSP 연동은 옮기지 않는다 (각 앱에 남는다).
