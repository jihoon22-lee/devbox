# Code Pad and Knowledge Base WSL workflows

## Overview

Issue #490의 Code Pad·Knowledge Base 범위와 두 앱의 Mermaid/bundle 계약을
정리했다. 앱 버전은 모두 `0.5.0`으로 맞췄다. WSL UNC를 일반 Windows UNC로
취급하지 않고, 편집·감시·LSP의 실제 capability를 분리했으며, WSL 연결 끊김과
불완전한 scan이 기존의 유효한 상태를 파괴하지 않도록 했다.

이번 범위에는 shell protocol, distro 내부 장기 실행 프로세스, 네트워크 fetch,
새 cross-app payload가 없다. 파일 bytes는 기존의 로컬 Windows/WSL filesystem
provider를 통해서만 이동한다. 실제 packaged Windows 앱에서 WSL distro를 연결하는
acceptance는 이 작업의 검증에 포함하지 않으며 #493에서 수행한다.

## Root cause and shared boundary

기존 흐름은 `\\wsl$\\<distro>\\...`와 `\\wsl.localhost\\<distro>\\...`를 일반
Windows 경로처럼 다뤘다. 그 결과 canonicalization이 반환할 수 있는
`\\?\\UNC\\wsl.localhost\\...` 표기와 alias가 같은 대상을 가리키지 못하고, Linux
경로의 `DevBox`와 `devbox`가 Windows식 대소문자 무시 비교로 합쳐질 수 있었다.
Windows host의 recursive notification은 WSL UNC provider에서 안정적인 계약이
아니며, host에서 실행하는 LSP도 WSL workspace를 지원한다고 주장할 수 없다.
추가로 WSL provider는 `ReplaceFileW`를 보장하지 않으므로 일반 Windows 원자 교체
경로를 그대로 사용할 수 없었다. 기존 watcher와 scan의 무제한 대기열/스냅샷은
이벤트 폭주나 부분 읽기를 deletion으로 오인할 위험도 있었다. 두 preview 컴포넌트가
Mermaid를 정적으로 import해 일반 editor 초기 bundle에도 큰 runtime이 들어가는
문제도 함께 정리했다.

공유 `crates/wsl` parser를 통해 다음 identity 규칙을 적용한다.

- `\\wsl$`, `\\wsl.localhost`, extended `\\?\\UNC\\wsl.localhost`를 하나의
  transport alias로 인식한다. transport와 distro 이름 비교는 ASCII
  case-insensitive다.
- Linux path tail은 Unicode와 대소문자를 그대로 보존한다. 공백·한글은 일반
  path data이며 shell quoting이나 명령 실행을 거치지 않는다.
- 빈 tail, traversal/control character, unsafe distro, oversize path는 거부한다.
- containment는 문자열 prefix가 아니라 component boundary로 판정한다.

## Changed files (grouped)

- Code Pad Rust: `apps/code-pad/src-tauri/src/commands/{file,folder}.rs`,
  `watcher.rs`, `lsp/manager.rs`, `lib.rs`. Workspace capability command,
  WSL polling, stable snapshots, bounded watcher state, host-LSP rejection,
  and WSL atomic replacement retry를 추가했다.
- Code Pad frontend: `src/App.tsx`, `api.ts`, `types.ts`,
  `components/{QuickOpen,LspControlPanel,PreviewPane}.tsx`와 regression tests,
  README. WSL/incomplete 상태와 LSP capability를 표시하고 preview를 lazy
  renderer로 전환했다.
- Knowledge Base Rust: `src-tauri/src/commands/{docs,watcher}.rs`,
  `core/{db,store}.rs`, `lib.rs`. canonical `VaultIdentity` 경계, native/WSL
  watcher lifecycle, bounded scan/apply와 status command를 연결했다.
- Knowledge Base frontend: `src/App.tsx`, `App.css`, `api.ts`, `types.ts`,
  `components/MarkdownPreview.tsx`와 테스트, README. WSL polling과 last-known-good
  상태를 sidebar에 노출하고 Mermaid lazy/stale-render 처리를 적용했다.
- Shared frontend/CI: 새 `packages/mermaid-renderer/{package.json,tsconfig.json,src/index.ts,src/index.test.ts}`와
  `.github/scripts/{check-frontend-bundles.mjs,test-check-frontend-bundles.mjs,frontend-bundle-budgets.json}`;
  두 앱의 Vite manifest 설정, package/lock metadata, workflow gate를 갱신했다.
- Release/configuration: 두 앱의 `package.json`, `src-tauri/Cargo.toml`,
  `tauri.conf.json`을 `0.5.0`으로 동기화하고, `windows-packaged-smoke-config.json`,
  `Cargo.lock`, `pnpm-lock.yaml`, `THIRD_PARTY_NOTICES.md`를 갱신했다. Smoke
  config에는 Knowledge `knowledge_watcher_status` shape probe를 추가했다.

`crates/wsl`의 path identity와 `crates/filesystem`의 filesystem identity,
`WalkResult.incomplete`, `atomic_write`는 이 구현이 소비하는 shared contract다.
이번 worktree의 앱 변경은 각 앱의 watcher lifetime과 mutation model을 유지한다.

## Code Pad WSL workspace

### Independent capabilities

`workspace_capabilities`는 canonical workspace path와 다음 payload를 반환한다.

```json
{
  "sourceKind": "wsl",
  "watchMode": "polling",
  "editSupported": true,
  "lspSupported": false,
  "lspReason": "host_lsp_wsl_unsupported"
}
```

WSL workspace도 파일 read/edit/atomic save는 local UNC provider를 통해 지원한다.
열린 파일의 외부 변경은 5초 주기의 bounded polling으로 감지하고, native workspace는
기존 `notify` parent-directory watcher를 사용한다. Quick Open은 최대 50,000개
파일의 한 번의 snapshot만 받고, `truncated`와 entry/metadata read failure를
나타내는 `incomplete`를 별도로 유지한다. frontend는 부분 목록·불완전 목록을
숨기지 않고 표시한다.

Windows host LSP는 WSL workspace에서 지원하지 않는다. frontend는 workspace를
선택할 때 `lspSync.setWorkspace(null)`로 분리하고 LSP checkbox/start/retry를
disable하며, 설정 화면에 “편집과 파일 감시는 지원하지만 Windows host LSP는 아직
지원하지 않는다”는 이유를 표시한다. Rust `LspManager`도 같은
`UnsupportedWslWorkspace`/`host_lsp_wsl_unsupported` 경계로 direct/manual start와
recovery journal 경로를 거부한다. WSL 안에서 LSP를 실행하는 별도 protocol을
shell command로 흉내 내지는 않는다.

### 5-second polling and bounded metadata snapshots

Code Pad watcher는 native callback을 application-lifetime worker로 보내고 한 곳에서
quiet-period debounce와 delivery를 처리한다. WSL file registration은 baseline
`StableSnapshot`을 저장한 뒤 5초마다 metadata를 먼저 확인한다. size, mtime,
filesystem identity가 이전 snapshot과 같으면 file bytes를 다시 읽거나 SHA-256을
계산하지 않는다. 변경이 의심될 때만 bounded read/hash를 수행하며, read 중 교체된
파일은 최대 3회의 짧은 stable-read 시도 후 event를 만들지 않는다.

| 경계 | 상한 | 초과/실패 동작 |
|---|---:|---|
| worker message queue | 1,024 messages | owning registrations의 snapshot 재확인 |
| one notify event | 256 paths | event를 신뢰하지 않고 재확인 |
| path key | 32 KiB | 해당 상태를 재확인 |
| pending debounce | 4,096 paths | overflow를 재확인으로 전환 |
| ready batch / live file registrations | 512 paths | 초과 registration을 고정 오류로 거부 |
| WSL polling | 5 seconds | bounded metadata probe |

callback error, channel/debounce overflow, 긴 path, polling diff의 문제는 조용히
버리지 않고 재확인 신호로 합쳐진다. generation은 unregister 후 queue에 남은 event가
동일 경로의 새 registration에 적용되지 않게 한다. 따라서 이벤트 폭주가 map이나
snapshot을 무제한으로 키우지 않는다.
live registration 자체도 512개로 제한하므로 fallback 재확인과 WSL poll에서 등록된
파일이 뒤로 밀려 영구적으로 누락되지 않는다. 감시를 시작할 수 없는 파일은 편집을
막지는 않지만, frontend가 외부 변경 감시를 사용할 수 없고 저장 시 snapshot 충돌
검사는 계속된다는 경고를 표시한다.

### WSL atomic save retry

일반 save는 같은 폴더에 고유한 `.code-pad-<pid>-<nonce>-<attempt>.tmp`를
`create_new`로 만들고 write/flush/sync한 뒤, 원본의 expected identity·mtime·size·hash를
교체 직전에 다시 검증한다. WSL target인 경우 Windows `ReplaceFileW` 대신
`MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`를 사용한다.
WSL UNC provider의 일시적인 `ACCESS_DENIED`, `LOCK_VIOLATION`,
`SHARING_VIOLATION`에는 최대 16회, 1/2/4/8/16ms로 제한된 짧은 backoff retry를
적용한다. 그 밖의 오류나 retry 소진 시 temporary를 정리하고 실패하며, non-WSL
Windows target은 기존 `ReplaceFileW` 경로를 유지한다.

## Knowledge Base WSL vault

### VaultIdentity and canonical root

`set_root`는 root creation preflight와 layout 확인 후 `VaultIdentity::inspect`를
수행한다. inspect는 기존 ordinary directory의 canonical path와 filesystem identity를
캡처하고, root object lease를 유지해 delete-and-recreate 뒤 identity 재사용을
오인하지 않게 한다. canonical path만 SQLite `root` setting에 저장하고 watcher도
같은 canonical value를 사용한다.

read/write/create/daily-note와 watcher path는 `existing_entry`/`new_entry`를 통해
root-relative traversal, outside path, symlink/reparse component를 거부한다.
`VaultIdentity::revalidate`는 canonical path와 identity를 다시 비교하며, scan 전후와
DB transaction commit 직전에 반복된다. scan 도중 vault가 다른 directory로 교체되면
revalidate가 실패하고 transaction은 commit하지 않는다. 이때 `vault_unavailable`을
보고하고 기존 SQLite rows와 last-known-good metadata를 보존한다.

앱 재시작 시 이미 저장된 root가 offline WSL distro 때문에 inspect되지 않더라도,
`restore_root`는 문법적으로 유효하고 32 KiB 이하인 WSL UNC만 보존한다. status는
`sourceKind=wsl`, `watchMode=unavailable`, `error=vault_unavailable`로 시작하고
worker가 5초마다 재연결을 시도한다. filesystem mutation은 distro가 돌아온 뒤에도
항상 새 `VaultIdentity::inspect`/revalidate를 통과해야 한다. native root나 malformed
path를 offline restore 후보로 보존하지 않는다.

### Native notify, WSL polling, and deletion authority

native vault는 recursive `notify`를 사용한다. WSL vault는 recursive Windows
notification 대신 5초 bounded metadata scan을 사용한다. 변경된 Markdown만
metadata stamp(size + modified nanoseconds)를 비교해 bounded UTF-8 content read를
수행하며, regular non-reparse file과 최대 10 MiB의 per-file limit을 지킨다.

| 영역 | 상한/주기 | 보장 |
|---|---:|---|
| watcher message queue | 1,024 messages | overflow는 root reconcile로 전환 |
| event payload | 256 paths, path 32 KiB | mutation event만 queue; access-only는 무시 |
| pending debounce / ready batch | 4,096 / 512 paths | bounded delivery |
| scan | 4,096 files, 4,096 directories | `vault_scan_limit`에서 deletion 보류 |
| scanned Markdown content | 64 MiB total, 10 MiB/file | before/open/after stamp가 같을 때만 index |
| WSL retry/poll | 5 seconds | offline root를 설정에서 제거하지 않음 |

incremental event는 upsert 후보를 만들 수 있지만 deletion authority가 아니다. scan이
`complete`일 때만 DB의 현재 path와 authoritative present set을 비교해 없는 docs를
삭제한다.

```rust
if scan.complete {
    // only here: rows absent from the authoritative scan may be removed
    remove_docs_not_in(&transaction, &scan.docs)?;
}
// incomplete/limit/unavailable scans retain prior rows
```

directory entry/metadata read failure, file disappearance during content read, scan/file
or directory limit, event overflow, root replacement, and offline distro all lower the
scan authority. Confirmed additions/updates may be applied in a transaction, but missing
paths remain in SQLite and the prior stamp map is extended rather than replaced. A
complete recovery scan is required before stale rows can be removed. A root replacement
detected after the DB transaction is prepared rolls the whole transaction back.

Successful changes write the privacy-safe integration snapshot and emit one
`docs-changed` event. The frontend separately receives `knowledge-watcher-status` and can
query `knowledge_watcher_status`; `KnowledgeWatcherStatus` includes `sourceKind`,
`watchMode`, `lastSyncedAt`, and stable errors (`vault_unconfigured`,
`vault_unavailable`, `vault_scan_limit`, `vault_scan_incomplete`,
`vault_index_failed`). The sidebar labels a healthy WSL vault as `WSL vault · 5초 폴링`
rather than an error. Offline, limit, partial-read, and index-failure states say that the
last index is retained and expose the last successful sync timestamp on hover.

### Atomic note saves

Knowledge `store::write_file` now delegates to `devbox_filesystem::atomic_write`.
The writer creates a uniquely named sibling temporary file, flushes/syncs its contents,
and atomically replaces the target. Command-layer path resolution is still gated by
`VaultIdentity`, so atomicity does not bypass root or reparse checks. The database upsert
and integration snapshot remain derived state; a failed/incomplete watcher scan never
turns a temporary provider failure into destructive document deletion.

## Shared lazy Mermaid runtime

`@devbox/mermaid-renderer` is now the only app-facing Mermaid dependency. It keeps the
dynamic `import("mermaid")` inside `getMermaidRenderer`, initializes once with
`startOnLoad: false`, dark theme, and `securityLevel: "strict"`, and shares one in-flight
promise across concurrent previews. A failed import/initialization clears that promise so
a later preview can retry. Mermaid is therefore absent from the initial editor chunk for
ordinary Markdown; the first actual Mermaid block (or standalone Code Pad Mermaid preview)
loads it on demand.

Both preview components guard async results with cancellation, current container identity,
and `element.isConnected`. A render that completes after the document/response changed is
discarded. Per-block last-good SVG remains cached; a syntax or renderer failure keeps that
SVG and adds the `mermaid-error-badge` instead of replacing it with empty/stale output.
The cache is cleared when switching documents, while concurrent blocks share the same
initialized runtime.

## Manifest-based initial bundle gate

Both app Vite configs enable `build.manifest`. CI builds the affected frontend, reads
`dist/index.html` module scripts, resolves each source through `.vite/manifest.json`, and
walks only the static `imports` graph reachable from those initial entries. It sums raw
bytes and deterministic gzip bytes (`mtime=0`). Other generated JavaScript is reported as
lazy chunks but excluded from the initial budget, so Mermaid's lazy runtime does not
silently consume the editor entry budget.

The checker fails closed for missing output/index/manifest, malformed manifest records,
duplicate entry/output, absolute/traversal or symlink path escape, missing static imports,
and raw/gzip budget overrun. `test-check-frontend-bundles.mjs` covers these failure modes,
including static import graph accounting and lazy-chunk exclusion. CI runs the fixture
tests in the dependency-policy job and runs the scoped checker after each frontend build.

| app | frontend tests | initial actual | checked-in budget |
|---|---:|---:|---:|
| Code Pad | 128 passed | raw `1,061,978` / gzip `360,133` bytes | raw `1,225,000` / gzip `415,000` bytes |
| Knowledge Base | 88 passed | raw `837,880` / gzip `280,603` bytes | raw `965,000` / gzip `325,000` bytes |

The Mermaid package itself also reports 3 tests and a successful TypeScript build. The
calibrated values above are initial entry totals, not the aggregate size of all lazy chunks.

## Verification evidence

Captured affected-workspace evidence:

- Code Pad Rust: core/unit 193, `lsp_client` 3, `lsp_manager` 15, and `lsp_process` 6
  tests — all passed.
- Knowledge Base Rust: 129 tests, including the offline-startup `restore_root` regression
  — all passed.
- `cargo check` and `cargo clippy -- -D warnings` — passed for the affected Rust targets.
- Code Pad frontend test/build and Knowledge Base frontend test/build — passed with the
  bundle measurements above; Mermaid renderer tests/build — passed.
- Dependency policy and focused tests (`check-dependencies.py`,
  `test-check-dependencies.py`), frontend bundle fixture tests, packaged smoke config,
  Windows installer acceptance config, catalog consistency, and `git diff --check` — all
  passed.

No Windows `tauri dev`/packaged runtime execution, installer installation, or physical
WSL distro file-change/reconnect run is claimed here. The actual packaged Windows+WSL
acceptance, including the offline/reconnect paths, remains the explicit #493 gate.
