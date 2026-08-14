# Repo Manager 설계 — Git Worktree Manager

- 상태: 제안(Proposal) — Stage 5
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §15.6, §17.9

## 1. 제품 정의

지정 root 아래 Git repository를 탐색해 브랜치·worktree·상태를 목록화하고,
worktree 생성과 열기를 제공한다. 파괴적 동작(force delete·reset·clean)은 기본 제공하지 않는다.

## 2. MVP

- 지정 root 아래 Git repository 탐색
- branch·dirty·ahead/behind·worktree 목록
- worktree 생성
- Code Pad·WSL Desktop·Workbench로 열기
- merged/stale branch 후보
- remove 전 uncommitted/untracked 검사

## 3. 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않는다.
- worktree remove 전 uncommitted/untracked를 확인하고 안내한다.
- Windows/WSL path가 같은 repository를 중복 등록하지 않는다 —
  `crates/wsl`(devbox_wsl) `canonical_project_key` 재사용 (§7.1).

## 4. 아키텍처

```
apps/repo-manager/
├─ src-tauri/src/
│  ├─ core/
│  │  ├─ git.rs        # git 출력 파싱 (branch·dirty·ahead/behind, 순수)
│  │  └─ discover.rs   # root 아래 repository 탐색 (순수: .git 존재 판정)
│  └─ commands.rs      # scan_root, repo_status, create_worktree, open_in
└─ src/
   ├─ App.tsx          # 저장소 목록·상태·worktree·열기
   └─ api.ts
```

## 5. 완료 조건

- root 아래 repository를 중복 없이 나열한다 (canonical identity).
- branch·dirty·ahead/behind·worktree 상태를 표시한다.
- worktree 생성과 열기가 동작한다.
- remove 전 검사가 동작하고, 파괴적 기본 동작이 없다.
