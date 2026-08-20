# repo-manager — Repo Manager (Git Worktree 관리)

지정 root 아래 Git 저장소를 탐색해 브랜치·worktree·상태를 목록화하고, worktree 생성·열기를 제공한다.
산출물: `RepoManager.exe` (`apps/repo-manager`).

## 주요 기능

- **저장소 탐색** — root 아래 Git repository 중복 없이 나열 (canonical identity)
- **상태 목록** — branch·dirty·ahead/behind·worktree
- **worktree 생성** — 새 작업 트리 생성
- **열기** — Code Pad에는 `Workspace`, WSL Desktop·Workbench에는 `Path`를 전달해 연다 (설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **정리 후보** — merged/stale branch 후보, remove 전 uncommitted/untracked 검사

## 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않음
- worktree remove 전 uncommitted/untracked 확인·안내
- Windows/WSL path가 같은 저장소를 중복 등록하지 않음 (`crates/wsl` canonical_project_key)

## 기술

- 공용 크레이트 `crates/wsl`·`crates/launch`(`open_in`)·`crates/filesystem`(`is_ignored_dir`, scan_root)
- git 출력 파싱·탐색은 순수 `core/` 로직

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-repo-manager-design.md`
