# Architecture

devbox는 **모노레포 + 다중 독립 앱** 구조를 취한다.

## 핵심 원칙

1. **하나의 저장소, 여러 독립 앱** — 각 앱은 독립적으로 실행되고 독립적으로 `.exe`를 만든다.
   모노레포는 개발 코드 관리 방식일 뿐 앱을 합치는 방식이 아니다.
2. **공통 코드는 실제 필요해졌을 때만 추출** — 첫 앱은 앱 안에 코드를 두고,
   두 번째 앱에서 같은 코드가 필요해지는 순간 `crates/`·`packages/`로 옮긴다.
3. **WSL에서 개발, Windows에서 빌드** — 순수 로직은 WSL에서 테스트하고,
   Tauri 앱 실행/배포는 Windows 툴체인으로 한다.

## 레이어

```
┌──────────────────────────────┐
│ apps/*   독립 Tauri 앱 (.exe) │
├──────────────────────────────┤
│ packages/*  React 공용       │  @devbox/ui, types, utils, config
├──────────────────────────────┤
│ crates/*    Rust 공용        │  process, wsl, database, filesystem,
│                              │  search, activity
├──────────────────────────────┤
│ 공통 인프라: Cargo workspace, │
│ pnpm workspace, git 모노레포   │
└──────────────────────────────┘
```

## 크레이트 의존 관계 (후보)

```
                     process ──┬─ port-manager
                     wsl ──┬───┴─ wsl-dashboard
    crates/process ◄───────┤
        ▲                   ├─ activity-timeline
        │                   └─ life-log
        │
  database ◄── activity-timeline, everything-plus, knowledge-base
 filesystem ◄── everything-plus, knowledge-base, life-log
    search ◄── everything-plus, knowledge-base
  activity ◄── activity-timeline, life-log
```

## 앱별 데이터 흐름

```
port-manager:    React → invoke → commands → process crate → OS netstat
wsl-dashboard:   React → invoke → commands → wsl crate → wsl.exe
activity-timeline: poller(tray 상시) → activity crate → SQLite → commands → React
everything-plus:  indexer/watcher → filesystem crate → search crate(FTS5) → React
knowledge-base:   fs_store → filesystem/search crate → React(CodeMirror)
api-playground:   React → commands → reqwest → HTTP
life-log:         React → commands → readers(타 앱 DB) → 집계 → React
```

## 통합 앱 (Workbench)

8개 앱 완성 후 `apps/workbench`를 추가한다. 기존 `crates/`·`packages/`를 재사용하므로
통합은 "새 앱 하나 + 메뉴 구성" 수준으로 끝난다. 결과물은 **독립 앱 8개 + 통합 앱 1개**.

상세 규약: [CONVENTIONS.md](../CONVENTIONS.md)
