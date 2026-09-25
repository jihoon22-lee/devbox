# AGENTS.md

Devbox는 Windows 11용 Tauri v2·React 19·TypeScript·Rust 모노레포다.
현재 소스는 네 제품(Workspace·API Studio·Knowledge·Control Center)이다. 공개 상태는 GitHub Release를 확인한다.
원격은 `https://github.com/jihoon22-lee/devbox`다.

## 먼저 읽을 규약과 작업 범위

- 공통 규약의 원장은 [CONVENTIONS.md](./CONVENTIONS.md)다. 변경 전에 §1·3·5·8·9의
  환경·스택·검증·Git 정책을 읽고, 작업에 해당하는 절을 추가로 읽는다.
- 대상 디렉터리의 AGENTS/override, `apps/<app>/README.md`, 해당 설계 문서를 확인한다.
  전체 README·과거 계획을 매번 읽지 않는다. 현재 구현은 코드와 테스트로 확인한다.
- 진행 중인 작업의 원장은 리뷰 후속 ledger 이슈와
  `docs/superpowers/plans/2026-09-23-review-remediation/00-roadmap.md`다. PR 단위·순서·게이트는
  로드맵 §4–§6을 따른다. v0.8 원장 #541·#542는 닫힌 역사 기록이다.
- 현재/과거 stable의 SHA·workflow·실기 근거는 [release evidence](./docs/release-evidence.md),
  앱·공용 모듈 목록은 CONVENTIONS §2와 [projects](./docs/projects.md)를 필요할 때 읽는다.

## 구현과 검증

- 패키지 매니저는 **pnpm 9**다. UI는 React/Vite·순수 CSS와 기존 `packages/tokens`를 사용한다.
- 순수 Rust 로직은 앱 `src-tauri/src/core/`, Windows 전용 처리는 command/platform 계층에 둔다.
  두 번째 실제 소비자가 생길 때만 `crates/`·`packages/`로 추출한다. 앱/crate 추가 시 Cargo
  workspace members와 필요한 카탈로그·검증 등록을 함께 갱신한다.
- WSL에서 Rust 사용 전 `source ~/.cargo/env`. 실제 앱 실행·배포 빌드는 Windows에서만 한다.
- **과제 단위로 필요한 테스트만 먼저 실행하고, 전체 검증은 PR 끝에 한 번 한다.** 과제를 구현할 때는
  실패하는 테스트를 먼저 쓰고 그 테스트와 직접 영향받는 테스트만 실행한다
  (`cargo test -p <crate> --lib <module>`, `pnpm --filter <package> exec vitest run <file>`).
  clippy·전체 build·`pnpm verify:affected`·Windows/WSL 실기 검증은 PR의 모든 과제가 끝난 뒤 모아 실행한다.
  `pnpm verify:all`은 release 준비·CI 검증기 변경·명시적 전체 감사에만 쓴다.
- 커밋은 과제 단위로 한다. push와 PR 생성은 PR의 상세 검증을 마친 뒤 한 번 한다
  (초안 PR은 CI를 실행하지 않는다). 완료 검증이 실패하면 확인된 수정을 먼저 모두 마치고
  실패·영향 범위만 다시 실행한다. 통과한 무관한 검사는 반복하지 않는다.
- 선행 PR에 의존하는 작업은 선행 PR 머지 후 시작한다. 파일이 겹치지 않는 독립 PR은 앞 PR의 CI를
  기다리는 동안 시작할 수 있다.
- 로컬 검증은 공통 자원 제한과 worktree 간 실행 잠금을 따른다. 전체 검증을 중복 실행하거나
  제한을 우회하지 않는다. 기본값·조정·측정은 [검증 운영](./docs/verification.md)을 따른다.
- 로컬의 기존 서비스·Docker·방화벽·공유 네트워크를 테스트 때문에 변경하지 않는다.
  새 WSL 배포판·별도 socket/data-root만으로 네트워크 격리를 인정하지 않는다. Docker 데몬
  설치/시작/중지, container/network 조작, iptables/nftables·라우팅 변경을 수반하는 검사는
  일회성 hosted CI 또는 검증된 독립 VM으로 제한한다. 환경 변수·옵션으로 차단을 우회하지 않는다.
- 사용자 데이터·secret을 fixture로 쓰지 않는다. v0.8 migration은 원본 보존·WAL consistent
  snapshot·destination namespace·재개/복구 경계를 검증한다. UI route 통합을 권한 통합으로 취급하지 않는다.

## PR·릴리스·정리

- 작업 시작 시 git status/worktree와 원격 작업에 필요한 인증 상태를 확인한다.
  브랜치는 CONVENTIONS §8, 커밋은 영어 Conventional Commits를 따른다.
- 커밋 완료와 PR 수용 완료를 구분한다. **main 머지 전에는 PR 최종 변경의 상세 검증과
  GitHub Actions CI 통과**가 필수다. 미실행 Windows 검증은 PASS로 보고하지 않는다.
- 릴리스 작업은 [release policy](./docs/release-policy.md)를 읽는다. exact-main 후보의
  assembly·packaged runtime·installer 검증 후 같은 commit의 stable만 승격한다.
  후보가 없거나 만료됐을 때 새 build로 대체하지 않는다. 명시 요청 없는 public RC는 만들지 않는다.
- 직접 만든 전용 worktree는 clean·merged 확인 → 제거 → `git worktree prune` → 로컬 브랜치
  삭제 → 원격 브랜치 삭제 순으로 정리한다. 활성·잠김·미머지·dirty·호스트 소유 worktree는
  삭제하지 않고 보고한다. 완료 전 worktree와 로컬·원격 브랜치 목록을 다시 확인한다.

## 작업 도구와 기록

- 일반 개발·migration 검토에 별도 스킬을 요구하지 않는다. 이 지침과 CONVENTIONS를
  직접 따른다. `.agents/skills/`에는 릴리스 전용 `devbox-release`만 유지한다.
- 작업 기록은 PR 본문과 ledger 이슈 댓글로 남긴다. 새 `workthrough/` 파일은 만들지 않는다.
  결정·영향·검증 결과·미실행 실기 항목을 PR 본문에 적고, 머지 후 ledger에 요약을 남긴다.
  컨텍스트 인계는 CONVENTIONS §11, 개인 설정은 [Codex setup](./docs/codex-setup.md)을 따른다.
