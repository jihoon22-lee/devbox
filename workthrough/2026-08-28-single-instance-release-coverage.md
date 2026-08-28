# Single-instance Release Coverage Completion

## Summary

The v0.5.0-rc1 source review found that Port Manager, Developer Toolbox, and Webhook Lab were the
only release applications without Tauri's single-instance plugin. That made the documented W4
contract impossible to pass for all 15 packaged applications: starting a second copy of any of
these three apps could leave an additional process and window instead of restoring and focusing the
existing main window.

This change completes the same behavior across those three apps and adds a catalog gate that rejects
future release apps when either the plugin dependency or its initialization is missing. It does not
change AppLink support or interpret command-line arguments for apps that do not advertise an AppLink
contract.

The already-published `v0.5.0-rc1` source tag remains immutable. RC1 is retained as a historical
candidate whose package acceptance cannot satisfy W4; this correction must be merged, packaged, and
fully revalidated as a new release candidate.

## Scope and behavior

The following release apps now install `tauri-plugin-single-instance` before their other Tauri
plugins:

| App | Previous second launch | New second launch |
|---|---|---|
| Port Manager | Could create another process/window | Shows, unminimizes, and focuses `main` |
| Developer Toolbox | Could create another process/window | Shows, unminimizes, and focuses `main` |
| Webhook Lab | Could create another process/window | Shows, unminimizes, and focuses `main` |

The callback deliberately ignores the second process's argument vector and working directory. These
apps do not declare AppLink inputs in `apps/catalog.json`, so treating arbitrary arguments as paths,
URLs, payloads, or commands would expand their public contract and create an unreviewed input path.
The implementation performs only idempotent window recovery operations; each operation is
best-effort so a transient window-state failure cannot panic either process.

Plugin initialization is first in each builder chain, matching the existing applications and
ensuring duplicate-process handling is registered before normal app setup. Existing local-data
migration, window restoration, clipboard/opener plugins, commands, and Webhook Lab server state are
unchanged.

## Regression gate

`.github/scripts/check-catalog.sh` already validates every `release: true` catalog entry against its
package, Rust, Tauri, identity, notices, and capability contracts. A ninth catalog invariant now
requires every release app to contain both:

1. an exact `tauri-plugin-single-instance = "2"` dependency declaration; and
2. `tauri_plugin_single_instance::init` in its Tauri library entry point.

Checking both sides catches the two common partial failures: declaring an unused dependency, or
copying initialization code without making the build reproducible. The check is catalog-driven, so
a future app becomes subject to the rule as soon as it joins the release matrix.

This static gate proves coverage is wired in source; it does not replace packaged Windows
acceptance. W4 must still launch the exact released executable twice and prove that one app process
and one main window remain, with the original window restored and focused.

## Dependency inventory

The plugin version was already locked for the other 12 applications. Regenerating `Cargo.lock`
therefore added the existing `tauri-plugin-single-instance` package to only the three private
workspace-package dependency arrays; no third-party version, source, checksum, or feature changed.

The canonical notice generator then updated only the `Cargo.lock` SHA-256 line in
`THIRD_PARTY_NOTICES.md`. The dependency inventory contents are otherwise byte-identical.

## Verification

The focused native checks were run from the dedicated worktree with two compile jobs at most:

```text
cargo check --offline -j 2 -p port-manager -p developer-toolbox -p webhook-lab
cargo test --locked -j 2 -p port-manager -p developer-toolbox -p webhook-lab
cargo clippy --locked -j 2 -p port-manager -p developer-toolbox -p webhook-lab \
  --all-targets -- -D warnings
cargo fmt --all -- --check
```

Results:

- Developer Toolbox: 51 tests passed.
- Port Manager: 27 tests passed.
- Webhook Lab: 77 tests passed.
- All binary targets and doctests passed.
- Focused compile and clippy completed without warnings.
- Workspace formatting and `git diff --check` passed.

The repository completion gates were then run against the complete workspace:

```text
cargo test --workspace --locked -j 2
cargo check --workspace --locked -j 2
pnpm install --frozen-lockfile
pnpm --workspace-concurrency=1 -r build
```

All Rust application, shared-crate, integration, binary, and doctest targets passed. All 15
applications and the shared context-menu typecheck produced successful frontend builds. The
existing Vite warnings for large API Playground, Code Pad, Developer Toolbox, Knowledge Base, and
WSL Desktop chunks and API Playground's mixed static/dynamic event import remained non-blocking;
this native-only correction introduced no new frontend module or warning.

Repository consistency checks also passed:

```text
bash .github/scripts/check-catalog.sh
python3 .github/scripts/check-dependencies.py generate
python3 .github/scripts/check-dependencies.py check
```

The first locked check correctly refused the stale lockfile before it was regenerated; this was an
expected dependency-lock safeguard, not a product failure. The offline regeneration succeeded
because the plugin was already present in the repository's dependency graph.

The pull request still requires the complete GitHub Actions matrix before merge. After merge, a new
immutable RC must pass independent release-asset verification and the full Windows W1-W4 packaged
matrix. Linux checks cannot establish Windows process exclusivity or focus behavior.

## Resource and cleanup handling

- Rust reused the existing Linux-native shared target and limited compilation to two jobs.
- No duplicate target directory, frontend install, Windows package, or release download was created
  for this focused correction.
- C: free space remained above the user-defined 100GB floor; no WSL compaction was attempted.
- The dedicated worktree and branch remain required until the correction PR is green and merged.
  They will then be removed in the repository-prescribed order.
- RC packages and validation scratch stay on E: and are retained only until their evidence has been
  incorporated. At final release closeout, superseded worktrees, release downloads, runtime scratch,
  stale worktree metadata/branches, and safely reproducible project caches will be removed so the
  repository can move to `/home/jihoon/projects` without carrying historical build state.

## Release boundary

This correction changes neither the v0.5.0 feature scope nor the stable-release claim. It closes a
release-contract gap discovered during RC review:

1. preserve RC1 and record its real package results;
2. merge this correction only after required CI passes;
3. prepare and tag RC2 from the corrected main commit;
4. verify all RC2 assets independently and execute W1-W4 on the exact packages;
5. proceed to stable preparation only after every mandatory acceptance row passes.
