# @devbox/tokens

devbox 앱이 공유하는 디자인 토큰 (CSS 커스텀 프로퍼티). 값은 기존 10개 앱의
`App.css`에서 실제 사용 빈도가 높은 것을 수집했다 — 새 팔레트를 발명하지 않는다.

## 사용

```css
/* apps/<app>/src/App.css 최상단 */
@import "@devbox/tokens/tokens.css";
```

## 토큰 목록

| 토큰 | 값 | 용도 |
|---|---|---|
| `--db-color-bg` | `#111418` | 창 배경 |
| `--db-color-surface` | `#171b21` | 패널/표면 |
| `--db-color-border` | `#262c36` | 경계선 |
| `--db-color-text` | `#e6e9ef` | 기본 텍스트 |
| `--db-color-text-muted` | `#8b93a1` | 흐린 텍스트 |
| `--db-color-accent` | `#4f8cff` | 강조/포커스 |
| `--db-color-danger` | `#f85149` | 위험/오류 |
| `--db-color-warn` | `#d29922` | 경고 |
| `--db-color-ok` | `#3fb950` | 성공 |
| `--db-space-1..6` | 4/8/12/16/20/24px | 간격 |
| `--db-radius-sm/md/lg` | 4/6/8px | 모서리 |
| `--db-font-sans` | Segoe UI... | 기본 폰트 |
| `--db-font-mono` | Cascadia Code... | 모노 폰트 |
| `--db-text-sm/base/lg` | 11/13/15px | 글자 크기 |
| `--db-focus-ring` | `0 0 0 2px rgba(79,140,255,.45)` | 포커스 링 |

## 규칙

- 토큰 값은 기존 앱에서 수집한 것이다. 새 값을 발명하지 않는다.
- 앱별로 의도적으로 다른 값이 필요하면 앱 로컬 변수로 남긴다 (토큰에 강제로 맞추지 않는다).
- 라이트 테마는 추후 `[data-theme]`로 덮어쓸 예정 (이름만 준비).
- React 컴포넌트·레이아웃은 이 패키지에 넣지 않는다.
