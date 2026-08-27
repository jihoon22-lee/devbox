# Devbox Launcher bounded bootstrap (#320)

## Overview

`origin/main`의 Repo Manager Git workflow 머지 커밋 `14716d0`에서 전용 worktree를 새로 만들고,
이전 Launcher 후보의 코드만 선별 이식한 뒤 최신 catalog·AppLink·문서 상태와 다시 통합했다.
이 작업은 범용 OS launcher가 아니라 devbox 앱과 제공된 integration snapshot을 한곳에서 찾고,
실행 직전 다시 검증해 기존 앱의 bounded AppLink로 넘기는 14번째 독립 Tauri 앱을 추가한다.

## Context and decisions

- 기존 후보는 오래된 main을 기준으로 하여 최근 완료 계획을 축소하는 문서 충돌이 있었다. 코드는
  새 worktree에 이식하고, 문서는 최신 상세 계획 위에 항목별로 다시 작성했다.
- Launcher가 지원하는 Workbench profile, Repo Manager repository, Everything+ saved query,
  WSL Desktop profile path는 consumer-side 계약이다. 아직 producer가 없는 path를 임의로 만들거나
  catalog에 제공 완료로 선언하지 않는다.
- 기존 Life Log→Knowledge `knowledge-draft/v1`은 구조화 one-time handoff로 유지한다. Launcher가
  이를 plain clipboard text로 노출하지 않으며, Developer Toolbox `toolbox-text/v1`도 실제
  claim/ack receiver가 준비되기 전에는 action으로 노출하지 않는다.
- app 설치·profile·task 같은 mutable 대상은 catalog/snapshot을 신뢰해 바로 실행하지 않는다.
  수신 앱이 embedded catalog 또는 현재 저장 상태를 다시 확인한다.

## Changes

### Bounded catalog and snapshot index

- build-time `apps/catalog.json`과 존재하는 versioned integration snapshot만 읽는 검색 index를
  추가했다. catalog revision을 8로 올리고 Launcher를 14번째 release app으로 등록했다.
- entry 수, query/path/id/text 길이, 전체 byte budget, 결과 수와 source 수를 제한하고 hidden app,
  self entry, unknown target, control character, secret-like source/payload를 거부한다.
- source 하나의 missing/stale/corrupt/permission/unsupported 상태가 다른 source 검색을 막지
  않는다. 동일 점수는 source·label·opaque id 순서로 정렬해 결과가 결정적이다.
- snapshot 결과는 실행 시 같은 source를 다시 읽고 entry·target·payload를 재검증한다. 삭제,
  교체, stale 전환 또는 payload 변경이 있으면 launch하지 않는다.

### Safe routing receivers

- `crates/applink`에 bounded `Task { id }`와 `Install { app_id }` routing target을 추가했다.
  둘은 장문 payload를 운반하지 않으며 기존 protocol v2 handoff envelope을 변경하지 않는다.
- Run Manager는 현재 저장된 job을 다시 찾고 확인 dialog를 표시한 뒤에만 실행한다. Cancel이 기본
  focus이며 Escape, Tab trap, trigger focus 복구와 승인 직전 task 재검증을 적용했다.
- Devbox Manager는 embedded catalog의 정확한 release app id만 선택하고 설치 화면을 연다.
  catalog에 없거나 Manager 자신·hidden app인 id는 거부한다.
- Workbench와 WSL Desktop의 `Profile`, 기존 `Path`, Everything+·Knowledge의 `Query`를 정확한
  target으로 전달한다. 지원하지 않는 새 target을 받은 기존 앱은 다른 기본 동작으로 fall
  through하지 않고 명시적 no-op/error로 끝낸다.

### Transient window, hotkey, and clipboard boundary

- 기본 `Ctrl+Alt+Space`와 제한된 대체 키를 native hotkey thread에서 등록한다. 설정 변경은
  저장 직후 재등록하고, 키 점유·미지원 플랫폼이면 초기 창을 숨기지 않은 채 안내한다.
- 검색 창은 hotkey로 표시·focus되고 focus loss 또는 Escape로 숨는다. 빈 결과에서 ArrowDown이
  잘못된 selection을 만들지 않으며, result list keyboard navigation을 제한한다.
- selected text 또는 clipboard는 사용자가 명시적으로 preview action을 선택한 동안에만 읽고
  modal에 표시한다. snapshot, catalog, history, log 또는 영구 저장소로 쓰지 않는다.

## Files

- `apps/devbox-launcher/` — React UI, bounded Rust index/commands, native hotkey와 Tauri packaging
- `crates/applink`, Run Manager, Devbox Manager, Workbench 및 기존 target consumer — bounded
  routing enum과 수신 처리
- `apps/catalog.json`, workspace manifests, dependency notices — 14번째 app 등록
- root/app README, architecture/development/roadmap/project/native-first/interop/Windows 문서 — 구현
  상태, producer 경계, privacy와 Windows checkpoint 동기화

## Verification

최신 main 기반 전용 worktree에서 다음 검증을 통과했다.

- `git diff --check`
- `bash .github/scripts/check-catalog.sh`
- `python3 .github/scripts/check-dependencies.py check`
- `pnpm install --frozen-lockfile` — 기존 pnpm store를 사용해 lockfile 변경 없이 완료
- `pnpm test` — 전체 frontend workspace 테스트 통과
- `pnpm build` — 전체 frontend workspace production build 통과
- `bash .github/scripts/run-frontend-scope.sh typecheck all` — 전체 TypeScript 검사 통과
- `cargo fmt --all -- --check`
- `cargo test -j1` — 전체 Rust workspace와 doc test 통과
- `cargo check -j1 --workspace`
- `cargo clippy -j1 --workspace --all-targets -- -D warnings`

실제 Windows `RegisterHotKey`, focus-loss hide, cold/hot AppLink, packaged installer와 Manager install
화면 이동은 v0.5.0 W3/W4 Windows checkpoint 대상이다. PR은 GitHub Actions의 Linux/Windows
compile·test·Clippy·frontend·catalog gate가 모두 통과한 뒤에만 merge한다.

## Scope and handoff

- 범용 file/web/Windows settings 검색, arbitrary shell, clipboard history, PowerToys plugin host,
  runtime external download를 추가하지 않았다.
- optional snapshot producer 구현과 `toolbox-text/v1` receiver는 각 후속 issue의 소유 범위로
  남긴다. missing producer는 정상적인 isolated source 상태다.
- Log Lens는 다음 신규 앱 #321에서 추가하며, 기존 13개 앱과 Log Lens의 window-state wiring은
  사용자 지정 묶음 #323–#336에서 처리한다.
