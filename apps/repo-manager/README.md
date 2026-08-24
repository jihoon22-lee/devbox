# repo-manager — Repo Manager (Git Worktree 관리)

지정 root 아래 Git 저장소를 탐색해 브랜치·worktree·상태를 목록화하고, worktree 생성·열기를 제공한다.
산출물: `RepoManager.exe` (`apps/repo-manager`).

## 주요 기능

- **저장소 탐색** — root 아래 Git repository 중복 없이 나열 (canonical identity)
- **앱 간 repository 선택** — catalog `Path`를 cold start와 실행 중 재호출에서 수신해 기존 항목을 선택하거나, 검증된 미등록 경로를 저장 전 초안으로 표시
- **상태 목록** — branch·dirty·ahead/behind·worktree
- **worktree 생성** — 새 작업 트리 생성
- **열기** — catalog에서 `path` capability와 실제 설치 executable이 모두 확인된 앱만 자동 노출하고, `workspace`도 받는 앱에는 더 구체적인 `Workspace` payload를 전달한다 (설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **정리 후보** — merged/stale branch 후보, remove 전 uncommitted/untracked 검사

## 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않음
- worktree remove 전 uncommitted/untracked 확인·안내
- Windows/WSL path가 같은 저장소를 중복 등록하지 않음 (`crates/wsl` canonical_project_key)
- inbound Path는 절대 경로·traversal·존재·Git repository 여부를 backend에서 검증하며, 실패 오류와 로그에 원문을 반향하지 않음
- 등록 초안은 자동 저장·Git 명령·임의 경로 쓰기를 수행하지 않고 사용자의 명시적 탐색 전까지 UI state로만 유지

## 기술

- 공용 크레이트 `crates/wsl`·`crates/launch`(`installed_targets`, `launch_open`)·`crates/filesystem`(`is_ignored_dir`, scan_root)
- git 출력 파싱·탐색은 순수 `core/` 로직

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-repo-manager-design.md`
