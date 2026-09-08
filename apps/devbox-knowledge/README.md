# Devbox Knowledge

Hidden v0.8 B03 product integrating the existing Notes, Activity and Search engines.
The public v0.7 topology remains unchanged until cutover. PR #556 / WP #545 tracks
remaining provider, search and runtime acceptance.

`pnpm --filter devbox-knowledge dev` opens the labelled browser fixture. On Windows,
`pnpm --filter devbox-knowledge tauri dev` runs the native product. Browser rendering
and portable tests are not Windows execution evidence.

The first native launch offers explicit new-store or legacy-import actions. A generation pointer
selects independent Notes, Activity and Search SQLite databases under the product's
installation-specific app-local root. Its private Markdown vault lives outside DB
generations. A corrupt/future pointer blocks initialization without resetting data.
No legacy source is initialized or imported by the new-user action.

Import reads the three known app-local legacy SQLite profiles with WAL-consistent
online backups. It copies settings/templates, activity/history and roots/saved
queries into an unselected generation, preserving current product data and indexes.
Original Markdown/assets stay in place. Source-table receipts remap IDs, reserve
deleted root references and preserve destination edits/deletions on repeat import.
The actual template, query and draft-history validators reject unsupported input.
Legacy collection consent is excluded; pending/sent deliveries become valid expired
history and never replay. Selecting an existing product's import setting schedules
review for the next launch without closing its dirty editors.

Preparation, cancellation, activation and recovery have a durable journal. Applying
requires a separate preview approval, unchanged source/destination snapshots and
three validated destination stores before selecting one pointer. Rollback compares
authoritative rows, permits derived-index changes and refuses newer user edits.
Both generations and all Markdown/assets remain preserved. Each snapshot is limited
to 256 MiB; row/cell/time limits, cancellation, disk preflight and eight retained
preparation slots bound the operation. Cancelled preparation slots can be reclaimed.

An imported legacy vault requires its original Windows database writer to close.
A retained share-read-only handle prevents that legacy writer from reopening, and
a shared exclusive vault lease prevents two product installations from writing the
same vault. Windows/WSL spelling aliases share the lease; Linux path case is preserved.
WSL availability checks and watcher restoration run away from startup, and note file
operations run away from the shared IPC executor. Per-write vault identity checks
remain in the existing engine.

The Notes folder setting schedules a review for the next start, preserving live
editors and the current binding. With engines stopped, preview inspects an existing
folder without creating anything; explicit apply acquires ownership, prepares only
fixed layout directories and atomically changes the binding/approval receipt while
clearing derived note indexes. Templates/settings and both vaults' files remain.
A crash between database commit and schedule cleanup is recognized by the receipt.
Five-second jobs, one retained worker and expiring previews bound remote probes;
cancelled late probes cannot change the binding. A missing native folder can be
replaced from startup without initializing the unavailable note engine. Unsupported
schemas and substituted previews fail closed. Unsafe live `set_root` stays blocked.

Git activity shares the same exact-range collector across calendar and digest/export.
Canonical common Git directory plus commit ID deduplicates linked worktrees; shared
commits are attributed to the first sorted configured path, while independent repos
remain separate. Unavailable projects remain visible and do not masquerade as zero
commits. Neither commit messages nor IDs enter a note summary.

The native registry separates note writing, Activity, Search reads, Search settings,
result opening and migration. Query calls cannot become note mutations or launches.
Every dispatch checks the local main window, native installation/session and replay
state. Component errors use fixed public codes rather than raw paths or user values.

Activity starts OFF without explicit product consent. Start/stop persists that
choice; process exit flushes the current privacy-filtered session without revoking
consent. Product autostart derives its own registry value from the native installation
identity and refuses foreign commands. Closing quits by default. An explicit
Activity setting hides to an available tray; it does not enable collection. The
tray's Quit action stops collection while preserving the next-launch consent.

Daily and Activity share the selected system-local civil date. Selecting a date
never creates a note. A missing Daily note requires a preview and one-time save;
an existing note opens through the editor's normal dirty-document confirmation.
Expired, cancelled or replaced previews cannot write, and a concurrent external
file is never overwritten. A derived-index failure preserves a published note.

Activity sends a deterministic selected-day digest directly to Notes through the
existing native one-time handoff store. Only an opaque draft reference crosses the
owner boundary; Activity cannot write the vault. Notes activates for a preview,
cancellation releases the claim, and explicit save publishes and consumes once.
The standalone Life Log launcher path remains available in the legacy app.

Visited feature routes remain mounted to preserve drafts. Hidden modal handlers do
not trap another route's focus. The shared shell styles only its direct containers.
Notes defers Activity, Search and Mermaid; the build checks its actual static closure
against the preserved 970,000 raw / 325,000 gzip legacy ceiling and separately checks
the 280,000 / 90,000 outer-shell budget.

Search provides Notes / Current Project / All Indexed Files on the existing full
filename/content/regex/filter/root/index/saved-search surface. Notes title/body FTS
and file FTS remain separate. File-only filters report unsupported in Notes;
Current Project uses a native registry projection and reports unavailable context
until the B04/B07 authenticated provider is connected.

The native source API returns opaque query/store generations, source/root identity,
result bounds and partial/stale/unsupported state. Read-only SQL has a 1.5-second
progress deadline; two retained workers per source prevent blocked filesystem calls
from spawning unbounded replacements. Four retained jobs and a 4 MiB candidate
projection bound storage; name/regex caps at 2,000, content at 200 and Notes at 100.
Known-offline roots retain inspectable index rows without probing. The UI applies
only the latest query. Product regex runs in a cancellable 500 ms worker.

Only rows with an issued opaque reference can open. The separate opener checks the
current store generation, registered/deepest root, indexed row and actual root/file
identity; raw renderer paths are rejected. Object leases prevent deleted/recreated
files from reusing a captured identity. They expire after three minutes or
cancellation; bounded background retirement keeps slow close operations off IPC
locks. Source-specific pools cap active and retiring object pairs at 8,000; folder
previews have a separate two-object pool. Two opener permits and a two-second response deadline bound slow
probes and prevent late launches. Notes results open through the editor's existing
dirty confirmation. Query services cannot launch a process or mutate a file.

The Windows foundation fixture covers startup, private note writes, default
collection OFF, independent Search and owner/replay/installation rejection in two
installations. The pinned legacy migration fixture additionally covers import,
source preservation, writer/installation denial, ID mapping, repeat/recovery,
summary preview/cancel/save/replay, regex deadline/recovery and opaque search opens.
The current Windows run has verified original data/import activation, collection
OFF, Activity summary preview/cancel/save/single consumption and regex timeout
recovery. Its Notes filename assertion has been corrected to use the filename;
a separate body query checks note FTS. Remaining search, repeat/recovery, folder
rebinding, configured performance and actual WSL acceptance are pending. The
[ownership/data map](../../docs/architecture/v0.8-knowledge.md) and B03 workthrough
separate portable, native and later suite evidence. B06/B04/B07 provider contracts
are described below; their authenticated external transports remain delegated.

### B06 session summary input

The native Notes component exposes `prepare_session_summary(bytes, expected)`;
its versioned [fixture](../knowledge-base/src-tauri/tests/fixtures/session-summary-v1.json)
is the B06 provider contract. `expected` comes from the native registered project,
worktree, execution target, session and revision. It is never renderer input. The
16 KiB strict DTO admits only an exact UTC interval, optional failed-run/Git counts
and at most eight selected problem categories. Unknown fields, invalid dates,
counts, duplicate categories and mismatched identities/revisions are rejected.
Unavailable counts remain unavailable. No command, log, path, window title,
problem message or secret is a summary field; opaque IDs are provenance only.

The provider publishes the deterministic result once as `knowledge-session/v1`,
from `devbox-workspace` to the private Notes consumer `knowledge-base`. B06 must
bind publication to its native operation and reuse that descriptor on retries,
revalidate the provider revision before publication, and deliver through the
product's authenticated IPC/owner route. Generic envelope names do not authenticate
an external sender. The current fixture adapter tests this receiver contract; the
actual Workspace provider/transport is implemented in B06.

`offer_product_draft` accepts only that native reference. Notes shows a fixed
snapshot for explicit preview/cancel/save through its existing one-time claim and
exclusive identity-checked new-note path. Existing files are not appended to or
overwritten. Once saved, redelivery cannot claim it again. The standalone legacy
store cannot preview this product-only kind. No new renderer mutation API is added.

### B04/B07 project provider

The native `install_project_snapshot`/`disconnect_project_provider` entry points
accept only a bounded projection from the authenticated registry owner; they are
absent from renderer commands. The [v1 fixture](src-tauri/tests/fixtures/project-provider-v1.json)
contains a registry revision, exact B01 current context, at most 256 project records,
normalized host roots, availability and explicit old Activity path mappings. The
whole projection is limited to 64 KiB; aliases are capped at 16 per record. B04/B07
must authenticate the provider, resolve opaque distro IDs to the supplied host
roots, forward revisions and revoke the projection on disconnect/expiry. Its actual
transport is their later integration scope. No renderer path becomes project authority.

Current Project now queries the same filename/content/filter engine with a native
folder prefix applied in SQL before LIMIT. It retains separate file/Notes indexes,
deepest-root ownership and case-sensitive WSL tails. A broad index root containing
multiple projects does not widen the selected scope. Cached rows are published
before any filesystem probe; offline/missing projects remain inspectable without
opening. Current Project and All Indexed Files share two retained file workers and
one object-retirement budget. A verified project root is pinned with each reference
and rechecked by the separate opener. Provider changes revoke old project jobs and
references without cancelling Notes or All Indexed Files; late old-context admission
cannot publish. Product events refresh the mounted Search/Activity views.

Activity keeps its old paths, durations and commit counts. A separate UI projection
marks mapped, unmapped, ambiguous, offline, missing or unavailable registry entries;
no row is silently dropped. Digest documents, exports and deterministic note bodies
are unchanged. Until the provider is connected, Current Project reports unavailable
context and Activity shows that the registry connection is pending.
