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
