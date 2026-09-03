# @devbox/mermaid-renderer

Code Pad와 Knowledge Base가 공유하는 lazy Mermaid runtime adapter다. package를 import하는 것만으로는
Mermaid chunk를 초기 bundle에 넣지 않고, 실제 diagram preview가 처음 필요할 때만 동적으로
불러와 한 번 초기화한다.

## Runtime contract

- `getMermaidRenderer()`는 성공한 initialization promise를 공유해 concurrent preview가 runtime을
  중복 초기화하지 않는다.
- import 또는 initialization이 실패하면 cached promise를 비워 다음 명시적 preview에서 재시도할
  수 있다.
- Mermaid는 `startOnLoad: false`, `theme: "dark"`, `securityLevel: "strict"`로만 초기화한다.
- `renderMermaid(id, source)`는 shared renderer의 결과 SVG를 반환할 뿐 DOM 삽입, source bound,
  diagram ID 생성, 오류 UI와 preview lifecycle은 소비 앱이 소유한다.
- package는 filesystem, network, Tauri IPC 또는 runtime download를 사용하지 않는다. Mermaid와
  이 adapter는 설치된 frontend asset에 함께 bundle된다.

## Consumers

| Consumer | Owned behavior |
|---|---|
| Code Pad | Markdown fence 탐지, diagram source/ID bound, preview rendering과 오류 표시 |
| Knowledge Base | note preview의 Mermaid fence 선택, source/ID bound, sanitize/render lifecycle |

새 소비자가 생기면 초기 static import graph에 Mermaid가 들어가지 않는지 앱별 Vite manifest와
bundle budget을 함께 검증한다. Mermaid version이나 security config를 소비 앱에서 개별 override하지
않는다.

## Development

```bash
pnpm --filter @devbox/mermaid-renderer test
pnpm --filter @devbox/mermaid-renderer build
```
