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
공유 계약은 product-contract·product-shell-tauri·suite-runtime·data-migration에 있다.
실제 workspace member와 의존성은 루트 Cargo.toml 및 각 package.json이 원장이다.

이전 사용자 데이터·Manager 설치 provenance를 읽는 `apps/legacy-v0.7-catalog.json`은
고정된 15개 원본 목록이다. 공개 제품 선택·새 실행 권한으로 사용하지 않는다.
[572개 기능 및 데이터 추적](v0.8-acceptance.md), [v0.7 역사적 목록](history/v0.7/projects.md).
