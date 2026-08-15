# 개발자 가이드

devbox는 Tauri 13개 데스크톱 앱을 하나의 모노레포로 관리하는 저장소다.

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
packages/    공용 React 패키지 (tokens·editor·diff-view)
crates/      공용 Rust 크레이트 (filesystem·integration·markdown·process·search·secrets·wsl)
docs/        architecture / roadmap / projects
```

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
- [docs/projects.md](./projects.md) — 앱별 요약
- [docs/windows-guide.md](./windows-guide.md) — Windows 사용/빌드 가이드
- `docs/superpowers/specs/` — 앱/기능 설계 문서 (workbench·webhook-lab·repo-manager 등)
- 각 `apps/<앱>/README.md` — 앱별 상세 소개
