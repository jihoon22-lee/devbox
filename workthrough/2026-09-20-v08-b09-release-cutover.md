# B09 — Four-product source and release cutover

Refs #551, #541, #542. Accepted base: B08 main `005b942d` / PR #561;
final CI `35496405510`, installed completion `35495003866` and scoped predecessors.
B01–B08 are complete. The initial plan below preceded implementation; final B09
source audit and CI findings are recorded at the end.

## Planned implementation

- Keep 14 consumed domain/host libraries under named `crates/` engines; remove
  their standalone Tauri startup/build/config/resources and all 15 old frontend
  shells. Move the retained global-shortcut adapter into Control Center. Preserve
  engines, migration readers, test fixtures and explicit legacy aliases.
- Freeze the v0.7 catalog for migration/reference readers before publishing only
  four products. Update Cargo/pnpm graph, metadata, budgets, capabilities and notices.
- Convert candidate/assembly/runtime/installer/promotion/read-back to four product
  ZIPs + Suite setup + manifest + notices. Keep exact-main identity and same-byte
  stable promotion; no public RC, no candidate substitution or skipped gates.
- Synchronize parity/data acceptance, R01–R26/S01–S08 traces and current docs.
  Retain historical facts and separate modeled/native/packaged/manual evidence.

## Execution rules

Finish the entire implementation/fixtures/docs before detailed verification.
Intermediate commits stay local. One final full audit and required CI; repeat only
actual failed/affected scopes. Hosted disposable Windows/WSL only for tests that
change Docker, services or networking. B09 cannot claim physical IME/multi-monitor
execution from CDP events. No production data migration during fixtures.

## Mechanical native extraction

Move consumed Rust sources, existing fixtures and manifests to domain engine paths;
redirect relative dependencies/includes and source consumers. The following semantic
commit removes the standalone bootstrap/configuration and obsolete app directories.
The frozen `apps/legacy-v0.7-catalog.json` is migration/reference input, never the
public runtime/release product list. The fake LSP server remains test-only.

## Source and release cutover implementation

- Removed all 15 standalone shells after extracting their consumed engines; frozen legacy
  catalog remains read-only source discovery/provenance. Public catalog now exposes exactly
  four products. Retained Launcher UI tests and existing engine/domain fixtures.
- Removed standalone engine Tauri bootstrap/build/resources and renamed Cargo packages,
  preserving Rust library aliases. Lockfiles/notices follow the current dependency graph.
- Direct Suite routing remains the product path; retired executable launch is centrally
  rejected before spawn. Historical metadata readers/argv fixtures remain for migration.
- Updated 572 feature records and data mappings with accepted B02–B08 source evidence.
  Git/vault original files, Manager install provenance remain in place. Derived caches,
  managed runtimes and live processes are not falsely marked as copied user data.
- Two Windows shards build four products, including static Workspace helper and Suite
  bootstrap. Private LSP fixtures stay outside public assets. Pinned NSIS assembles one
  setup plus four ZIPs, manifest and notices; exact file closures/digests/source are checked.
- Reconstruct installer acceptance input from verified public bytes; test exact original
  private payload byte identity, including LF on Windows. Stable promotion has no rebuild
  fallback. Published acceptance downloads again and exercises all four full ZIPs.
- Candidate native domains run independently on disposable Windows, WSL2/Docker separately.
  Seven pinned baseline anchors are measured on the same VM as matching product workloads;
  unchanged B01 budgets cover startup/input/idle/warm/search/task/profile readback.
- Windows Cargo jobs restored to two after locking only the shared Tauri build-script
  staging copy. Dependency cache namespace and failure retention remain in place.
- Current documentation now describes four products and migration/recovery; old v0.7
  guides are preserved under docs/history/v0.7. R01–R26/S01–S08 mapping and unexecuted
  physical IME/monitor/reboot layer limits are explicit in docs/v0.8-acceptance.md.

## Verification state

No detailed B09 checks ran during development. After implementation/fixtures/docs were
complete, the final audit started. Four product builds/budgets, additional TypeScript and
all frontend packages except one obsolete catalog-count assertion passed; the corrected
assertion passed in its focused rerun. Source/parity/data/package corruption/provenance
contracts passed. Rust check initially exposed dependency aliases after Cargo package
renaming; explicit original library aliases fixed the whole graph and check passed. Clippy
found two orphan comments after standalone deletion; both were removed together. Rust
unit compilation also exposed two removed legacy seed helpers; restored them only under
cfg(test), preserving one-shot/replacement regressions without a production argv writer.
The complete Clippy/all-target and Actions syntax gates passed. The full Rust test run
completed with only the frozen Manager catalog count assertion failing (19→15); that
exact assertion passed after correction. Three deliberately ignored tests remain ignored.
The complete frontend suite likewise had only its frozen count assertion corrected.
Final formatting and changed native-script syntax passed. Required CI and exact-main
native/package acceptance are still pending, not claimed PASS.
Successful unrelated scopes are retained instead of rerunning the full suite. Candidate
assembly/native/installer/WSL2 gates run only on exact current main after B09 source merge.
Final outcomes belong in #541/#542/#551 and Actions, without a result-only source PR.


## Windows CI finding and correction

CI 35503063755 / source ecb82069 passed Catalog, Dependency, Frontend and Linux
Rust, and Windows check/Clippy. Windows tests collected ten failures, all from the
same embedded Node lock digest: its LF .gitattributes rule still referenced the
removed Code Pad path. Move the rule to crates/editor-engine/src/lsp/node-lock.json;
keep the reviewed digest/data unchanged. Add a metadata assertion for the attribute
and pinned digest so future relocation cannot wait until native tests to reveal this.
No product logic changes and no repeat local frontend/Rust suite are needed for this
checkout-byte correction. Final-source required CI remains mandatory.

## Exact candidate findings — bounded B09 correction

PR #562 merged after final CI 35504681097 passed. Exact-main candidate 35505536649
at d88cb623 assembled all seven public assets; installer migration/update/recovery/
removal passed. The candidate is not accepted for promotion while native scopes fail.

- Three native scopes stopped before product execution because baseline subset
  configuration was a PSCustomObject passed to a dictionary-only report writer.
  Preserve all frozen budgets/nested values and explicitly convert its top-level
  properties to an ordered dictionary before writing.
- The native WSL short task produced its cwd file but no matcher diagnostic. The
  supervisor could execute and exit before the host's /proc identity probes. Gate
  user execution on a bounded stdin acknowledgement after exact identity validation;
  missing/wrong acknowledgement cannot execute the task. Keep task stdin closed and
  preserve all buffered output. Add supervisor regressions for short exit/status/logs
  and rejected acknowledgement, and distinguish native exit 3 from handshake failure.
  The native fixture records bounded stdout/stderr on any remaining matcher failure.
- Complete these corrections and their fixtures/docs before running the failed and
  affected scopes together. Retain unrelated successful B09 local checks. Required
  final-source CI and a fresh exact-main candidate remain necessary for changed code.
- Actual Windows 11 two-monitor 96-DPI window move and restart-position retention
  passed on the original candidate. Native IME input was blocked by Windows foreground
  ownership and is not reported as passed. No Docker/service/network/display changes.
- Cross-product evidence retained after cancelling the already failed candidate shows
  cold Knowledge activation failed after successful approval/project/Review scenarios.
  Suite resumed its listener during plugin setup, before Tauri creates the main window
  and shell state. Resume on RunEvent::Ready instead. The fixture now adopts a cold
  child even when command delivery fails, closes the old instance gracefully, and
  prints stage/completion records so a failed child cannot silently keep the job alive.
- Runtime's isolated package compilation exposed its missing explicit Tokio `net`
  feature, previously supplied by workspace feature unification. Declare its actual
  dependency. Runtime regressions: 302 passed, one deliberately ignored. Baseline
  PowerShell parsing and all three subset serializations passed with unchanged budgets;
  performance JS contracts passed. Retain these results after the separate Suite fix.

Final affected Clippy/all-targets passed for Runtime and all four consuming products.
Changed JS syntax and diff checks passed. Required PR CI and fresh candidate remain.

## Final Source first-frame correction

PR #563 passed CI 35509894449 and merged as 1f09afaa. Candidate 35511259083
passed four-product assembly, API, Knowledge, cross-product and full WSL2/Docker
acceptance, including the previously failing cold activation and short WSL task.
Product-shell acceptance exposed a Source UI first-frame race: immediately after
approving a new worktree, HistoryDiffPanel's passive initialization could invalidate
the first history request, leaving the panel empty with no error. Initialize and
retire repository state in a layout effect, before the frame accepts input. A
regression clicks in the first committed frame on mount and repository change;
the native fixture keeps its immediate first click and its assertions unchanged.
Preserve passing unrelated checks, collect this frontend correction's completion
checks, and require final CI plus a fresh exact-main candidate for changed UI bytes.
The same candidate screenshot also exposed the obsolete unconditional native
“development build” badge. Remove that badge from native products; retain the
explicit browser/mock-data notice. This is a label correction, with no route,
authority, feature or native-code changes.

History correction completion: verify:affected passed the Workspace build/bundle,
772 workspace-feature tests and 91 product tests, including the first-frame regression.
The subsequent native badge-only JSX correction passed product-shell TypeScript;
its consuming products are covered by final PR CI. No repeated Runtime/Rust checks.
Installer 35511259083 also passed; product-shell Source remains that candidate's only
failed scope. The candidate is not eligible for stable promotion.

## Terminal retirement correction

PR #564 passed CI 35514330280 and merged as 658a25bd. Candidate 35514551027
passed Source in both installations, installer, API, Knowledge and cross-product.
Initial WSL2 ownership observation returned unavailable; a single scoped retry
with identical bytes passed the full WSL2/tmux/Zellij/Docker scope. Preserve the
original failure rather than claim a root cause that its report did not retain.
Product-shell attempt 1 completed the first installation's native WSL tasks but
timed out on the second; attempt 2 exposed a companion reopen with one pane.
No further blind retries are planned.

The native owner marked a window stopping but still accepted layout autosaves
while PTYs closed one by one. Those closed events could persist a shrinking
layout before the window was destroyed. Freeze layout writes under the same
owner lock once retirement begins, including app shutdown. Revalidate the
current window session after worker admission and reject requests from replaced
peers. Keep preparing-generation writes valid for initial hydration. Add a real
metadata regression retaining two panes across late autosave, stop and replacement.
The native fixture asserts retained pane keys immediately on reopen; bounded
failure reports now retain layout/session and WSL operation/run/log state before
cleanup. No deadlines are relaxed and no assertion or safety gate is removed.

Finish this correction, fixture and documentation before completion checks. Keep
successful unrelated scopes; require final CI and a new exact-main candidate for
the changed native code. Existing local Docker/services/network remain untouched.

Terminal correction completion: `pnpm verify:affected` passed Workspace Rust
format/check/Clippy/tests (182 tests), including the real metadata retirement
regression. Changed JavaScript syntax and diff checks passed. Frontend code was
unchanged and not rerun; authoritative dependency audit runs in required CI.
Final required PR CI and the new release-byte candidate remain.
