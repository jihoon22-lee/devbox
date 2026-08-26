# Life Log #305 export 보강 workthrough

## 목적

`feat(life-log): Markdown·JSON·CSV export`의 dirty draft를 PR 전 계약에 맞게
보강했다. 이 작업은 v0.5.0 P2-10의 date-range export만 다루며 Knowledge handoff,
cloud/LLM 전송, 새 DB schema, 외부 GUI를 추가하지 않는다.

## 확정 경계

- Windows native 경로는 DB와 검증된 local integration snapshot을 읽고, 명시적으로
  확정한 save dialog 경로에만 atomic write한다.
- 브라우저 경로는 Tauri/DB에 접근할 수 없다는 사실을 `origin: browser-preview`와
  모든 source의 `browser_preview_only` 상태로 표현한다. native 저장 성공이나 가짜
  producer version/generatedAt을 만들지 않는다.
- 날짜 시작은 inclusive, `endMs`는 exclusive다. frontend가 system-local civil day별로
  계산한 `dayBoundaries`를 authoritative range input으로 전달하고, native가 고정
  24시간 또는 다른 timezone으로 다시 계산하지 않는다.
- session은 `start_ts >= startMs && start_ts < endMs`만 포함하고 저장된 duration을
  자르지 않는다. 제목·앱·privacy 규칙·credential marker를 export 경계에서 다시
  bounded 처리한다.

## 구현 계약

### Git

- 공용 `crates/filesystem::parse_safe_project_path`를 사용해 POSIX/Windows drive/UNC
  absolute path만 허용하고 root, relative, traversal, device alias를 거부한다.
- unique project는 최대 64개로 제한하고 common path identity로 중복 제거한다.
- 각 저장소에 `git -C <path> --no-pager log --since=... --before=... --format=%ct --`
  fixed argv를 한 번만 실행한다. shell quoting/remote write/hook은 사용하지 않는다.
- Git의 초 단위 filter 결과를 다시 millisecond `[startMs,endMs)`로 필터링한 다음
  authoritative day boundary에 귀속한다. stderr/remote URL/credential/path error는
  export 밖으로 보내지 않는다.
- 공용 bounded runner는 2초 timeout, 256KiB stdout, child kill/wait와 stable error
  code를 보장한다. malformed timestamp나 partial parse는 해당 repository count를 0으로
  만들고 해당 project row에 error code를 붙인다. Git source도 unavailable 처리해 partial
  결과를 전체 성공으로 표시하지 않으며, 정상적인 0 commit repository와 실패를 구분한다.

### Snapshot/provenance

- 준비 단계에서 producer별 가장 높은 validated snapshot version을 하나 선택하고,
  더 높은 version이 손상/미지원이면 낮은 version으로 silent fallback하지 않는다.
- `schemaVersion`(envelope schema), `snapshotVersion`, `producerVersion`,
  `generatedAt`, `freshnessMs`, `view`, `scope`를 source metadata에 고정한다.
- Run Manager는 validated flat snapshot, Knowledge는 `activity/v1` named view를
  사용한다. 구 flat Knowledge payload는 `legacy-data`로 명시한다.
- snapshot을 발견한 뒤 다시 읽을 때 producer/version/generatedAt identity가 변하면
  `snapshot_changed_during_read`로 격리한다. 현재 snapshot은 요청 날짜 범위에 해당하는
  history가 아니므로 `latest-snapshot-out-of-range` provenance row만 남기고 summary에
  activity처럼 합산하지 않는다. note ID/title/body/raw environment는 export하지 않는다.

### Format/save

- JSON은 typed `ExportDocument`로 serialize/deserialize하고, CSV는 24열 고정 header와
  quote-aware CRLF parser, Markdown은 고정 marker를 저장 직전에 재검증한다.
- 저장 전 source 검증은 source ID별 schema/snapshot/provenance/view/error 관계까지 다시
  확인한다. Git error를 snapshot source에 넣거나, 실패 project에 non-zero commit을 넣거나,
  project별 error와 aggregate error 목록이 어긋난 artifact는 거부한다.
- Markdown table cell, JSON, CSV RFC 4180 escaping을 적용하고 source/error 순서와
  app/path 정렬을 고정한다.
- native save는 validated absolute path와 matching extension만 허용하며, 기존 target
  symlink/non-file를 거부하고 `filesystem::atomic_write`를 호출한다. 취소·parser·OS
  오류는 고정 메시지만 반환한다.

### UI

- 한 번에 하나의 export만 허용하고 request token/ref로 늦게 도착한 결과가 notice/error/
  dialog state를 덮어쓰지 못하게 한다.
- 브라우저 다운로드 직전에 origin/format/mime/byte length를 재검증하고 object URL을
  정리한다. 성공 notice도 `브라우저 미리보기로 다운로드`라고 구분한다.
- range modal은 labelled dialog, busy state, initial focus, Escape close, Tab trap,
  restore focus, disabled date/format/save/cancel을 제공한다. modal backdrop가 배경 조작을
  막고 완료 notice는 실행 시작 시점에 고정한 format을 사용한다.

## 상한 및 fail-closed 목록

| 항목 | 상한/규칙 |
| --- | --- |
| 날짜 | 1–366 civil days; boundary 0보다 크고 최대 48시간 |
| session | 50,000 records; app 256 bytes; title 4 KiB |
| privacy | JSON 64 KiB; 각 rule 128개; regex 512 bytes |
| project | 64 unique paths; path 4 KiB |
| Git | 2초/process; stdout 256 KiB; fixed safe error code |
| snapshot digest | Run service 256개/ID 256 bytes/uptime 100년; Knowledge ID 512개/ID 128 bytes; metric count 10억 |
| output | 4 MiB; JSON/CSV/Markdown 저장 전 재검증 |

## 검증 기록

- 추가한 순수 fixture는 exact half-open Git range, malformed output rollback,
  privacy regex bound, credential title redaction, snapshot version selection,
  Knowledge activity view/freshness, JSON roundtrip, CSV fixed width를 점검한다.
- PR 전 재검토에서 Markdown cell의 역슬래시·backtick·pipe 조합을 함께 escaping하고,
  React unmount 시 export request token을 무효화해 pending native/browser 결과가 detached
  UI를 갱신하지 않도록 보강했다. 프론트 fixture는 local calendar boundary(DST에서 고정
  24시간을 가정하지 않음), browser-preview의 네 source unavailable/고정 error code,
  24열 CRLF CSV, malformed timezone의 비반향, export 실패의 고정 안내, modal initial
  focus/양 끝 Tab trap/busy double-action 차단/unmount stale completion을 고정한다.
- 최종 핵심 검토에서 손상된 session의 음수 duration·역전된 end를 조용히 clamp하던 경로를
  제거하고 fixed error로 fail-closed했다. Git failure는 aggregate code뿐 아니라 정확한
  project row에 결합하고, JSON 저장 전 source별 provenance/error 관계와 project/aggregate
  error 일치를 재검증한다. Browser input/day boundary도 native DTO처럼 exact field 집합을
  요구하며 modal의 date/format 입력까지 pending 동안 잠근다.
- Git bounded runner의 `stdin=Stdio::null()`/stderr discard 계약을 재확인하고, Windows
  path identity가 같은 여러 표기(`C:/...`, `C:\\...`, 대소문자 차이)는 deterministic
  display path 하나만 남기는 fixture를 추가했다. browser Markdown도 native와 같은
  pipe/backslash escaping을 사용하도록 맞췄다.
- `cargo test -p life-log -p git -j2` — Life Log 68 tests와 shared Git 7 tests passed.
- 별도 Linux-native `/tmp/devbox-life-log-export-mirror.*` mirror(기존 의존성 링크만
  사용, install 없음)에서 `vitest run --passWithNoTests --config vite.config.ts` — 4 files,
  31 tests passed. mirror는 검증 직후 제거했다.
- 같은 방식의 별도 mirror에서 `tsc -p tsconfig.json --noEmit` — passed. 이 mirror도
  확인 직후 제거했다.
- 최종 PR 전 게이트에서 `cargo test --workspace -j2`, `cargo check --workspace -j2`,
  `cargo clippy --workspace --all-targets -j2 -- -D warnings`,
  `cargo fmt --all -- --check`가 모두 통과했다. 이 결과에는 Life Log 68 tests와 shared
  Git 7 tests뿐 아니라 전체 Rust workspace test suite가 포함된다.
- Linux-native 고정 mirror에서 루트 `pnpm test`와 `pnpm build`가 통과했다. Life Log는
  4 files/31 tests, API Playground 13 files/114 tests, Code Pad 14 files/113 tests 등
  전체 frontend workspace test suite가 통과했고 모든 앱의 TypeScript/Vite production
  build가 완료됐다. Code Pad·Knowledge Base·WSL Desktop의 기존 chunk-size warning은
  실패가 아니며 이 export 변경으로 새로 발생한 경고가 아니다.
- dependency policy와 그 regression tests, build-manifest notice tests,
  release-note extraction tests, catalog check, `git diff --check main...HEAD`가 통과했다.
  Windows native dialog/atomic replace와 실제 Git for Windows 경로는 CI 또는 Windows
  W2에서 별도 검증해야 한다.
- PR #425의 첫 Windows Rust 1.98 Clippy run은 Windows에서만 compile되는 save-success
  block 끝의 `return`을 `needless_return`으로 보고했다. expression tail로 고친 뒤 Life Log
  68 tests, Linux Clippy `-D warnings`, format check를 다시 통과했다. WSL의 Windows GNU
  cross-Clippy는 source lint에 도달하기 전에 host에 MinGW C compiler가 없어
  `libsqlite3-sys` build 단계에서 중단됐으므로, 최종 Windows 결과는 새 PR CI SHA에서
  확인한다.

## rebase/checkpoint

- 기존 초안과 후속 hardening commit을 최신 `main` `0495971`에 rebase한 뒤 하나의
  Conventional Commit으로 soft-squash했다. 최종 PR branch는 하나의 feature commit만
  포함한다.
- rebase 과정에서 공용 bounded Git stdin/null·credential-shaped project path omission,
  app value redaction, duplicate Run service ID rejection, Windows dialog helper/test cfg를
  변경하지 않았다. PR을 열기 전 위 전체 게이트를 다시 수행해 rebased 결과를 검증했다.

## 수정 파일

- `apps/life-log/src-tauri/src/core/export.rs`
- `apps/life-log/src-tauri/src/commands/export.rs`
- `apps/life-log/src-tauri/src/core/db.rs`
- `apps/life-log/src-tauri/src/core/mod.rs`
- `apps/life-log/src-tauri/src/commands/mod.rs`
- `apps/life-log/src-tauri/src/lib.rs`
- `apps/life-log/src-tauri/Cargo.toml`
- `crates/git/src/lib.rs`
- `apps/life-log/src/api.ts`
- `apps/life-log/src/App.tsx`, `apps/life-log/src/App.css`
- `apps/life-log/src/App.test.ts`, `apps/life-log/src/App.contextMenu.test.tsx`,
  `apps/life-log/src/api.export.test.ts`
- `apps/life-log/src/lib/contextMenu.ts`, `apps/life-log/src/lib/contextMenu.test.ts`
- `apps/life-log/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
