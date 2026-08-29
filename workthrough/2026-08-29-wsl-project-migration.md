# Workthrough: WSL-native project migration

**Date:** 2026-08-29

## Summary

devbox v0.5.1 main source를 `origin/main`에서 `/home/jihoon/projects/devbox`로 새로 clone하고,
기존 local stash를 보존했다. 정리 완료된 old worktree root는 숨김 항목까지 비어 있음을
확인했으며 새 ext4 위치에 빈 `devbox-worktrees` root만 준비했다.

## Changes

### 1. Source and local Git state

- source와 target initial HEAD는 tag `v0.5.1`의
  `300cb158d1f0c23973857549a1aeddd9997c3f16`으로 일치한다.
- 기존 stash 1개(`5141eac6ed1324052eafe204bd868bcaddb6bd2d`)와 reflog entry를 target에
  복원했으며 apply하지 않았다.
- ignored `logs`, dependency/build output과 cache는 옮기지 않고 target에서 재생성한다.
- source와 target의 `devbox-worktrees` entry count는 모두 0이다.

### 2. Path contract

- current source location은 WSL에서 `/home/jihoon/projects/devbox`, Windows toolchain에서
  `\\wsl.localhost\Ubuntu\home\jihoon\projects\devbox`이다.
- `E:\WSL\Ubuntu`는 VHD storage 위치일 뿐 Windows source checkout 경로가 아니다.
- path canonicalization test fixture와 과거 workthrough/release evidence의 `/mnt/e/projects` 및
  `E:\projects` 예시는 의도된 historical/compatibility input이라 일괄 치환하지 않았다.
- production profile은 application data에 저장되므로 repository의 browser-only mock fixture를
  실제 runtime configuration으로 취급하지 않았다.

## Testing

- source/target HEAD, stash object와 clean worktree 비교: PASS
- old/new worktree root entry count: 0/0
- target filesystem 확인: ext4
- Windows PowerShell에서 WSL UNC source와 root `Cargo.toml` 접근: PASS
- `pnpm@9 test`, `pnpm@9 build`: PASS
- `cargo test --workspace --locked`: PASS
- `cargo check --workspace --locked`, `cargo fmt --all -- --check`: PASS

## Files Modified

- `CONVENTIONS.md` — 현재 WSL/Windows source 경로와 VHD/source 구분
- `workthrough/2026-08-29-wsl-project-migration.md` — 이관·stash·worktree 경계와 검증 기록

## Notes

- stash에는 미완료 변경이 있으므로 사용자가 처리하기 전 삭제하거나 apply하지 않는다.
- 원본 프로젝트와 WSL VHD backup은 target 검증 및 사용자 승인 전까지 삭제하지 않는다.
