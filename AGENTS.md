# AGENTS.md

Devbox는 Windows용 Tauri v2·React 19·TypeScript·Rust 모노레포다.
현재 공개 v0.7.0의 15개 앱을 유지하며 v0.8.0의 네 제품 통합을 준비한다.
원격은 `https://github.com/jihoon22-lee/devbox`다.

## 먼저 읽을 규약과 작업 범위

- 공통 규약의 원장은 [CONVENTIONS.md](./CONVENTIONS.md)다. 변경 전에 §1·3·5·8·9의
  환경·스택·검증·Git 정책을 읽고, 작업에 해당하는 절을 추가로 읽는다.
- 대상 디렉터리의 AGENTS/override, `apps/<app>/README.md`, 해당 설계 문서를 확인한다.
  전체 README·과거 계획을 매번 읽지 않는다. 현재 구현은 코드와 테스트로 확인한다.
- v0.8 원장 [#541](https://github.com/jihoon22-lee/devbox/issues/541), 수용 기준 #542,
  실행 계획 #543~#551을 따른다. **v0.8에서는 CONVENTIONS §8의 B01~B09 통합 PR 정책이
  일반 기능별 PR 규칙보다 우선한다.** #541/#542를 구현 PR에서 자동으로 닫지 않는다.
- 현재/과거 stable의 SHA·workflow·실기 근거는 [release evidence](./docs/release-evidence.md),
  앱·공용 모듈 목록은 CONVENTIONS §2와 [projects](./docs/projects.md)를 필요할 때 읽는다.

## 구현과 검증

- 패키지 매니저는 **pnpm 9**다. UI는 React/Vite·순수 CSS와 기존 `packages/tokens`를 사용한다.
- 순수 Rust 로직은 앱 `src-tauri/src/core/`, Windows 전용 처리는 command/platform 계층에 둔다.
  두 번째 실제 소비자가 생길 때만 `crates/`·`packages/`로 추출한다. 앱/crate 추가 시 Cargo
  workspace members와 필요한 카탈로그·검증 등록을 함께 갱신한다.
- WSL에서 Rust 사용 전 `source ~/.cargo/env`. 집중 검증은 대상의 `pnpm build/test` 또는
  `cargo test/check -p <package>`. 실제 앱 실행·배포 빌드는 Windows에서만 한다.
- 기본 완료 검증은 루트 `pnpm verify:affected`다. commit·staged·unstaged·untracked와
  역의존 소비자를 포함한다. resolver가 all을 선택하면 전체 검증한다.
  `pnpm verify:all`은 release 준비·CI 검증기 변경·명시적 전체 감사에 사용한다.
- 사용자 데이터·secret을 fixture로 쓰지 않는다. v0.8 migration은 원본 보존·WAL consistent
  snapshot·destination namespace·재개/복구 경계를 검증한다. UI route 통합을 권한 통합으로 취급하지 않는다.

## PR·릴리스·정리

- 작업 시작 시 git status/worktree와 원격 작업에 필요한 인증 상태를 확인한다.
  브랜치는 CONVENTIONS §8, 커밋은 영어 Conventional Commits를 따른다.
- 커밋 전 집중 검증 + affected 검증, **main 머지 전 PR 최종 변경의 GitHub Actions CI 통과**가 필수다.
  미실행 Windows 검증은 PASS로 보고하지 않는다.
- 릴리스 작업은 [release policy](./docs/release-policy.md)를 읽는다. exact-main 후보의
  assembly·packaged runtime·installer 검증 후 같은 commit의 stable만 승격한다.
  후보가 없거나 만료됐을 때 새 build로 대체하지 않는다. 명시 요청 없는 public RC는 만들지 않는다.
- 직접 만든 전용 worktree는 clean·merged 확인 → 제거 → `git worktree prune` → 로컬 브랜치
  삭제 → 원격 브랜치 삭제 순으로 정리한다. 활성·잠김·미머지·dirty·호스트 소유 worktree는
  삭제하지 않고 보고한다. 완료 전 worktree와 로컬·원격 브랜치 목록을 다시 확인한다.

## 작업 도구와 기록

- 저장소 스킬은 `.agents/skills/`다. 일반 변경은 `devbox-change`, migration/권한/복구 검토는
  `devbox-migration-review`, 명시적인 릴리스 작업은 `devbox-release`를 사용한다.
- 작업 기록은 PR 묶음당 workthrough 하나를 갱신한다. 결정·영향·검증·남은 작업만 적는다.
  상세 운영과 컨텍스트 인계는 CONVENTIONS §11, 개인 설정은 [Codex setup](./docs/codex-setup.md)을 따른다.
