# @devbox/a11y

Devbox 데스크톱 앱의 최소 접근성 계약을 공유한다.

- `@devbox/a11y/styles.css`: 키보드 포커스, 강제 색상, 모션 축소 기본값
- `@devbox/a11y`: IME-safe 키 처리와 dialog 포커스 유틸리티
- `@devbox/a11y/testing`: jsdom에서 실행하는 axe 구조 검사

페이지 배경이나 레이아웃은 정의하지 않으므로 투명 Tauri 창에도 안전하다. 색 대비는
jsdom에서 측정할 수 없으므로 Windows의 고대비 수동 acceptance에서 별도로 확인한다.
