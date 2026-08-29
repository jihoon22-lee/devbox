# 개발자 가이드

devbox는 현재 v0.5.1 안정판의 15개 Tauri 데스크톱 앱을 하나의 모노레포로 관리한다.
v0.5.1은 #470/#473/#477/#478/#479를 포함하며, 정확한 tag commit·workflow·asset publication
metadata는 GitHub Release에서 확인한다.

- **pnpm workspace** — `apps/*`, `packages/*`
- **Cargo workspace** — 앱(src-tauri) + 공용 crates
- **공통화 원칙** — 두 번 이상 실제로 필요해진 코드만 `crates/`·`packages/`로 추출

## 구조

```
apps/        독립 Tauri 앱 (각각 독립 .exe)
  port-manager        Port & Process Manager
  developer-toolbox   개발 도구 모음
  wsl-desktop         임베디드 WSL 터미널 (distro/Docker/프로젝트 패널, wsl-dashboard 흡수)
  api-playground      REST API 테스트
  everything-plus     로컬 파일 검색
  knowledge-base      마크다운 지식 저장소
  life-log            자동 일일 로그 (활동 추적 흡수)
  devbox-manager      devbox 앱 설치·업데이트·실행
  code-pad            CodeMirror 6 경량 코드 에디터 (LSP)
  run-manager         예약 실행·서비스 관리
  workbench           프로젝트 기반 orchestration 셸
  webhook-lab         로컬 웹훅/콜백 서버
  repo-manager        Git worktree/저장소 관리
  devbox-launcher     catalog app·integration snapshot launcher와 explicit clipboard preview
  log-lens            bounded local/WSL/container log viewer (v0.5.0 bootstrap; #473 Run reader in v0.5.1)
packages/    공용 React 패키지 (tokens·editor·diff-view·context-menu)
crates/      공용 Rust 크레이트 (applink·catalog·filesystem·git·integration·launch·markdown·process·search·secrets·window-state·window-state-tauri·wsl)
docs/        architecture / roadmap / projects
```

## 기능·의존성 판단

- 외부 도구에 같은 기능이 있다는 이유만으로 개발 대상에서 제외하지 않는다.
- 반복적인 P1·P2 흐름은 대상 자체가 WSL/Git/remote API인 경우를 제외하면 network와 별도
  runtime 설치 없이 완료되어야 한다.
- permissive library는 출처·version·license·크기·보안·오프라인 동작을 검토해 설치물에
  포함할 수 있다.
- 대형 전문 도구 설치·실행은 native 기능을 대체하지 않는 optional integration으로만 둔다.
- devbox 앱 간 전달은 clipboard/file export보다 versioned applink·handoff·snapshot을 우선한다.

상세 정책은 `CONVENTIONS.md` §9, v0.5.0 history와 v0.5.1 maintenance 범위는
[v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md)을 따른다.
stable은 exact annotated `vX.Y.Z` tag 또는 명시적 manual dispatch 뒤 release workflow가
성공하고, 15-app/32-asset/31-declared/mismatch-0 및 Windows evidence를 독립 확인한 뒤에만
주장한다. prerelease/RC는 사용자의 명시 요청 없이는 만들지 않는다.

## 시작하기

```bash
corepack enable pnpm        # 최초 1회
pnpm install                # 워크스페이스 의존성
```

앱 실행/빌드는 각 앱 디렉터리에서:

```bash
cd apps/port-manager
pnpm tauri dev              # Windows에서
pnpm tauri build            # Windows에서 (배포)
```

WSL에서는 `cargo test`(core 로직) / `pnpm build`(프론트 검증)로 개발한다.

## 참고 문서

- [CONVENTIONS.md](../CONVENTIONS.md) — 공통 규약 (스택, 개발 워크플로, git 규칙)
- [docs/architecture.md](./architecture.md) — 아키텍처
- [docs/roadmap.md](./roadmap.md) — 로드맵 / 진행 상황
- [v0.5.0 네이티브 우선 계획](./superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md) — P1·P2·선택 P3 상세 계획
- [docs/projects.md](./projects.md) — 앱별 요약
- [docs/windows-guide.md](./windows-guide.md) — Windows 사용/빌드 가이드
- `docs/superpowers/specs/` — 앱/기능 설계 문서 (workbench·webhook-lab·repo-manager 등)
- 각 `apps/<앱>/README.md` — 앱별 상세 소개
