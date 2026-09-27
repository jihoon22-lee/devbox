# 제품과 모듈

현재 소스의 사용자 제품은 `apps/devbox-workspace`, `apps/devbox-api-studio`,
`apps/devbox-knowledge`, `apps/devbox-control-center` 네 개다. 공개 제품 목록은
`apps/catalog.json`, typed route/component/authority 목록은 `apps/products.json`이 원장이다.
모든 제품과 Suite version은 0.8.1이다. 공개 여부는 [Release](https://github.com/jihoon22-lee/devbox/releases)와 [#541](https://github.com/jihoon22-lee/devbox/issues/541)를 따른다.

| 소유 제품 | 프런트엔드 | 보존한 native engine |
|---|---|---|
| Workspace | workspace-features | projects-engine, editor-engine, repositories-engine, runtime-engine, ports-engine, logs-engine, terminal-engine |
| API Studio | api-studio-features | http-client-engine, webhook-host, toolbox-engine |
| Knowledge | knowledge-features | knowledge-vault-engine, content-index-engine, activity-engine |
| Control Center | control-center-features, product-shell/launcher | installation-tools, 제품 내부 platform/hotkey |

engine은 독립 실행 앱이 아니며 Tauri bootstrap·installer·공개 카드를 갖지 않는다.
순수 로직은 engine의 `core/`, Windows 처리와 command adapter는 host 계층에 남는다.
공유 계약은 product-contract·product-shell-tauri·suite-runtime에 있다.
실제 workspace member와 의존성은 루트 Cargo.toml 및 각 package.json이 원장이다.

이전 15개 앱과 데이터 전환의 근거는 비공개 보관소에 보존한다.
[572개 기능 및 데이터 추적](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/v0.8-acceptance.md), [v0.7 역사적 목록](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.7/projects.md).

Suite의 native bus·installation/peer identity·health 구현은 `crates/suite-runtime`이
소유하며 네 제품이 명시적 Cargo 의존성으로 사용한다. 제품별 command admission과
domain handler는 각 host에 남는다. Control Center 앱 소스를 `#[path]`로 포함하지 않는다.

Knowledge의 WSL 문서 저장은 `workspace-wsl`의 명시적 Cargo protocol 의존성과 동일 정적
helper artifact를 사용한다. 각 제품이 별도로 해시를 고정해 패키징하며, Knowledge의 진입점은
파일 연산만 제공한다. Workspace의 project/task/session authority는 공유하지 않는다.

공유 OS 구현은 `crates/process-tree`(자식 프로세스 트리)와 `crates/secrets`(DPAPI)에 있다.
소비자는 기존 종료 기한·오류·용도별 entropy를 소유한다. `workspace-wsl`의 소스는
`crates/wsl-helper`에 있고 패키지·실행 파일 이름과 제품별 resource 경로는 유지한다.

Knowledge·Workspace의 살균된 Markdown HTML과 Mermaid 블록은 `packages/markdown-view`를
공유한다. 노트 링크·wikilink·앵커 이동과 각 제품의 미리보기 상태는 소비자가 소유한다.

`crates/agent-protocol`은 사용자별 agent의 길이 제한 framing·메시지·handshake 검사를
제공하는 순수 계약 crate다. Cargo workspace 검증에 포함되며 제품 카탈로그 항목은 아니다.
프로세스·배포·writer lease 연결·종료·복구 경계는 [ADR 0015](adr/0015-devbox-agent.md)를 따른다.

`apps/devbox-agent`는 Control Center가 배포하는 창 없는 내부 구성요소다.
검증된 설치 owner의 namespace와 writer lease를 사용하며 공개 제품 카탈로그에는 넣지 않는다.
portable에서는 실행하지 않는다. runtime·webhooks·동의한 activity·search index와 트레이·로그인 설정을 소유한다.

`crates/workspace-core`는 Workspace와 agent가 공유하는 프로젝트 host·registry·저장소 선택·Git/WSL 실행 증거 계층이다.
agent의 읽기 전용 host는 registry owner 잠금을 가져가거나 파일을 만들지 않으며,
쓰기 owner의 원자적 변경을 매번 다시 읽는다. 제품 UI의 파일·터미널 수명은 Workspace에 남는다.

`crates/knowledge-stores`는 Knowledge와 agent가 같은 generation manifest를 읽는 계층이다.
생성·선택은 Knowledge가 수행하고 agent는 activity/search 경로와 읽기 전용 수집 동의만 확인한다.

`crates/mcp-server`는 stdio MCP의 JSON-RPC·버전 협상·도구 계약을 검증하는 순수 crate다.
파일·네트워크·Tauri를 사용하지 않고 실제 작업은 native ToolHost가 수행한다.
Cargo workspace와 의존성 기반 affected 검증에 포함되며 공개 제품이 아니다.
