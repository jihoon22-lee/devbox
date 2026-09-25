# 개발자 가이드

pnpm 9·React 19·TypeScript·Rust·Tauri v2를 사용한다. 앱은 네 `apps/devbox-*`, UI는
`packages/`, native engine과 공유 계약은 `crates/`에 있다. [제품 목록](projects.md)을 참조한다.

1. WSL에서 편집·Git·frontend 개발을 하고 Rust 사용 전 `source ~/.cargo/env`를 실행한다.
2. `pnpm install --frozen-lockfile`로 의존성을 설치한다. 개발 UI는 필요한 제품에서 `pnpm dev`로 연다.
3. 실제 Tauri 실행·배포 빌드는 Windows에서 한다. 예: `pnpm --filter devbox-workspace tauri dev`.
4. PR 전체 기능·importer·fixture·문서를 먼저 끝낸다. 개발 중에는 diff·범위·최소 문법만 확인한다.
5. 개발 완료 후 루트 `pnpm verify:affected`를 한 번 수행한다. root/lock/검증기 변경과 release 준비는
   `pnpm verify:all`이다. 포함 검사를 따로 중복하지 않는다. 실패 수정은 모아서 해당 범위만 재실행한다.
6. 최종 push 후 required CI를 통과시킨다. 중간 커밋은 로컬에 모아 불필요한 자동 CI를 피한다.

로컬 검사는 [자원 제한과 잠금](verification.md)을 따른다. 기존 Docker·방화벽·공유 네트워크와
서비스를 바꾸지 않는다. WSL 배포판만 새로 만드는 것은 네트워크 격리가 아니다. 그런 검사는
일회성 hosted VM에서만 실행한다. 제품 native fixture도 실제 사용자 profile 대신 소유한 임시
경로 또는 존재하지 않음을 확인한 native namespace를 사용하고 cleanup 근거를 남긴다.

Windows Cargo job은 2개다. 네 제품 build.rs는 공유 Tauri staging 복사 구간만 file lock으로
직렬화한다. 전체 Rust 빌드를 직렬화하지 않으며 CI 캐시는 실패해도 dependency codegen을 보존한다.

source 전환 전 설명은 [v0.7 개발 기록](https://github.com/jihoon22-lee/devbox-archive/blob/main/docs/history/v0.7/development.md)에 보존한다.
