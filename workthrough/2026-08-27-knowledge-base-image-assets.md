# Knowledge Base image paste/drop assets (#304)

## Overview

Knowledge Base Markdown editor에 이미지 붙여넣기와 드롭을 추가했다. 사용자가 별도 이미지
도구를 찾거나 파일을 수동으로 옮기지 않아도, 명시적인 paste/drop 한 번으로 vault 내부의
content-addressed asset과 노트-relative Markdown image node를 만들 수 있다. OCR, 외부 image
hosting, clipboard history와 같은 기능은 추가하지 않았으며, 기존 note 원문은 사용자가
Save를 선택하기 전까지 native command가 직접 수정하지 않는다.

## Context and decisions

- Issue #304(P2-09)는 `assets/` content-hash 저장, relative Markdown link, vault boundary,
  collision, partial-write 방어를 요구한다. #303 quick capture와 같은 cohesive PR에
  포함하지만, 이 file의 acceptance/tests는 image slice로 독립 유지한다. `VaultIdentity`,
  no-replace publication, and identity-checked cleanup are shared instead of introducing a
  second path-only writer.
- 지원 형식은 PNG/JPEG/GIF/WebP로 고정했다. MIME과 사용자가 제공한 filename을 신뢰하지
  않고 native magic/header 판정 결과로 확장자를 결정한다.
- 이미지 bytes는 frontend와 native 모두에서 2 MiB로 제한한다. native는 한 변 16,384px,
  총 64M pixel, JPEG header scan 64 KiB 경계를 추가 적용해 비정상 metadata와 메모리
  폭주를 fail-closed한다.
- 파일은 `assets/<sha256 lowercase>.<png|jpg|gif|webp>`로만 생성된다. 같은 hash와 동일
  bytes만 재사용하고, collision·symlink·non-file target은 덮어쓰지 않는다.
- 저장은 bounded temp file을 create-new으로 만든 뒤 write/flush/sync한다. Unix는 hard-link와
  부모 directory `fsync`로, Windows는 destination을 교체하지 않는
  `MoveFileExW(..., MOVEFILE_WRITE_THROUGH)`로 완성 파일을 atomic하게 노출한다. quick
  capture와 동일한 helper가 no-replace 경계를 소유한다.
- note path와 `assets/`는 `VaultIdentity`의 canonical Knowledge root와 filesystem identity에서
  다시 검사한다. absolute/drive/UNC,
  traversal, control 문자, root 밖 symlink/reparse는 고정 오류로 차단한다.
  Windows volume/file identity가 확인되지 않는 경우에도 fail-closed하며, target rollback
  identity가 불명확하면 path-only 삭제를 수행하지 않는다.
- frontend는 keyboard/browser paste event와 drag/drop을 처리하고, context menu Paste를
  명시적으로 선택한 경우에만 `navigator.clipboard.read()`를 시도한다. Tauri clipboard
  permission은 기존 `allow-read-text`에 머문다. WebView에서 image Clipboard API를 사용할 수
  없으면 기존 text paste로 fallback한다.
- native 성공 응답은 generated relative path, generated Markdown, `reused`만 전달한다.
  frontend는 응답 shape를 다시 검증하고 요청 당시 document text와 stable note identity를
  확인한 뒤 CodeMirror 초안에 삽입한다. busy/double action, IME, unmount, note 전환,
  document 변경 중 도착한 응답은 삽입하지 않는다.

## Changes made

### Native policy and storage

- `apps/knowledge-base/src-tauri/src/core/assets.rs`
  - `ImageFormat`, fixed `AssetError`, content hash naming, note-relative link 생성기를
    추가했다.
  - PNG/JPEG/GIF/WebP magic과 bounded dimension parser를 순수 함수로 분리했다.
  - 2 MiB bytes, 16,384px dimension, 64M pixel, 64 KiB JPEG header 경계를 적용했다.
  - note/asset path validation은 user filename이나 raw error를 결과에 포함하지 않는다.
  - supported fixtures, malformed/oversized/dimension/path negative cases를 추가했다.

- `apps/knowledge-base/src-tauri/src/commands/assets.rs`
- `save_image_asset` Tauri command와 camelCase/unknown-field rejection DTO를 추가했다.
  note path와 base64 envelope도 bounded serde visitor에서 먼저 거부한다.
  - bounded canonical base64 decode, `VaultIdentity`-checked existing Markdown note 확인,
    fixed vault `assets/` directory 검증과 생성, collision-safe publish를 구현했다.
  - quick capture가 소유한 no-replace publication과 identity-checked cleanup helper를
    공유하고, configured root만 읽어 default-root mutation을 피한다. Unix parent directory
    `fsync`와 Windows `MOVEFILE_WRITE_THROUGH` 경계는 공용 publication helper가 담당한다.
  - temp file의 partial write를 오류 시 정리하고, 기존 동일 bytes만 `reused`로 반환한다.
    기존 content-addressed target 비교도 metadata/read race에서 `MAX_ASSET_BYTES + 1`로
    bounded하게 읽어 경쟁 파일 교체가 메모리 사용량을 키우지 못하게 했다. stage 이후
    stale/reparse 실패는 temp identity를 확인한 cleanup으로 되돌린다.
  - collision, same-content idempotency, unsafe/missing note, assets symlink escape, fixed
    error non-echo fixtures를 추가했다.

- `apps/knowledge-base/src-tauri/src/commands/markdown.rs`
  - 기존 preview loader가 nested note의 `../assets/...`를 안전하게 해석하도록 POSIX path
    normalization을 추가했다.
  - normalization 이후 canonical existing entry로 다시 확인해 root 밖/absolute/Windows
    drive/UNC/control path와 symlink entry를 읽지 않는다.
  - nested asset 및 unsafe image source fixtures를 추가했다.

- `apps/knowledge-base/src-tauri/src/commands/mod.rs`, `core/mod.rs`, `lib.rs`
  - 새 policy/command module을 등록하고 `save_image_asset`을 invoke handler에 연결했다.

### Frontend input and editor integration

- `apps/knowledge-base/src/lib/imageAssets.ts`
  - DataTransfer image filtering, bounded File read, chunked base64 conversion, relative
    destination 계산, native response shape validation, fixed error mapping을 구현했다.
  - image bytes와 generated path/Markdown가 bounds를 벗어나지 않도록 frontend mirror를
    제공한다.

- `apps/knowledge-base/src/api.ts`
  - 명시적인 context-menu image read를 위한 `readClipboardImage`와 native
    `saveImageAsset` API를 추가했다.
  - native 결과는 frontend에서 다시 `assets/<hash>.<safe-ext>`와 exact Markdown shape로
    검증한다.

- `apps/knowledge-base/src/components/MarkdownEditor.tsx`
  - CodeMirror paste/dragover/drop event에서 단일 image를 가로채고, non-image text와 IME는
    기존 입력 pipeline에 남겼다.
  - 현재 editor document, selection, stable document key를 snapshot하고 async 완료 후
    재검증한다.
  - busy state, role=status, aria-busy, focus restore, stale/unmount guard와 multi-file
    fixed error를 추가했다. 좌표 mapping을 사용할 수 없는 초기 WebView/jsdom layout은
    현재 selection으로 안전하게 fallback한다.

- `apps/knowledge-base/src/App.tsx`, `src/types.ts`, `src/App.css`
  - Markdown note에서만 import callback을 연결하고, save 전 in-memory draft에 generated
    node를 삽입한다.
  - image status overlay와 generic `role=alert` 오류 표시를 추가했다.

- Existing test mocks in `src/App.test.tsx`, `src/App.applink.test.tsx`,
  `src/App.wikilinks.test.tsx`에 새 API surface를 반영해 기존 앱 fixture가 실제 모듈 shape와
  일치하도록 했다.

### Documentation

- `apps/knowledge-base/README.md`: 사용 흐름, supported format/size, hash naming, relative
  link, atomic/collision/symlink 경계, Save와 비범위를 상세화했다.
- `docs/architecture.md`: Knowledge data flow, asset storage boundary, preview normalization,
  clipboard permission/privacy 경계를 architecture와 security section에 추가했다.
- `docs/roadmap.md`: P2-09 image draft의 구현 상태와 포함/제외 범위를 기록했다.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`: P2-09의 format,
  bounds, path, atomic publish, stale/accessibility fixture와 W2 계획을 구체화했다.
- No new Rust/TypeScript dependency, lockfile, sidecar, network service, persistence schema,
  or clipboard permission was introduced.

## Code examples

### Content-addressed native response

```rust
// apps/knowledge-base/src-tauri/src/commands/assets.rs
let hash = assets::content_hash_hex(bytes);
let relative_path = assets::asset_relative_path(&hash, format)?;
let markdown = assets::markdown_link(note_rel, &relative_path)?;
let reused = match publish_asset(&vault, &relative_path, &target, bytes)? {
    PublishResult::Created => false,
    PublishResult::Reused => true,
};
```

### Stale-safe editor insertion

```typescript
// apps/knowledge-base/src/components/MarkdownEditor.tsx
const documentBefore = view.state.doc.toString();
const documentKeyBefore = documentKeyRef.current;
const asset = await callback(file);
if (
  !mountedRef.current ||
  viewRef.current !== view ||
  documentKeyRef.current !== documentKeyBefore ||
  view.state.doc.toString() !== documentBefore
) {
  onErrorRef.current(IMAGE_STALE_ERROR);
  return true;
}
view.dispatch({ changes: { from, to, insert: asset.markdown } });
```

## Verification results

The exact cohesive #303/#304 tree was reviewed and gated in the dedicated
worktree. Cargo used a Linux-native target directory and two build jobs, and
Vitest used two workers, to avoid `/mnt/e` compile I/O and resource spikes.

### Rust

```text
cargo test -p knowledge-base -j2                    PASS — 100 tests
cargo check -p knowledge-base -j2                   PASS
cargo clippy -p knowledge-base --all-targets -j2 -- -D warnings
                                                     PASS
cargo fmt --all -- --check                         PASS
```

### Frontend

```text
pnpm --filter knowledge-base exec vitest run --maxWorkers=2
                                                     PASS — 11 files / 68 tests
focused MarkdownEditor/quick-capture/API regression
                                                     PASS — 25 tests
pnpm --filter knowledge-base build                 PASS — tsc + Vite, 2,156 modules
git diff --check                                   rerun on the committed PR tree
```

The final review additionally bound asset reuse and Markdown preview reads to
the opened file's identity, added vault checks around temp-file writes,
charged every regular file against watcher reconciliation, and covered Tauri
string rejection redaction and jsdom drop-coordinate behavior. GitHub Actions
remains the authoritative clean dependency and Windows compile gate.

## Remaining verification and risks

- Windows W2 packaged smoke is still a release-gate task: real WebView2 clipboard image paste,
  Explorer file drop, nested-note preview, locked/partial storage and Save behavior must be checked
  with evidence.
- `navigator.clipboard.read()` is capability-dependent in WebView2. When unavailable, explicit
  menu Paste safely falls back to text; keyboard paste/drop still use browser events.
- Native image inspection intentionally validates bounded static raster headers rather than decoding
  or transforming pixels. The preview path continues to use the existing sanitized/data-URI
  renderer; OCR, conversion and external hosting are intentionally out of scope.
- The asset can exist as an unreferenced content-addressed file if the user changes note/document
  state after native publish but before editor insertion. The document is never changed in that
  stale case; orphan cleanup is a separate future maintenance decision and is not silently added
  to this issue.
