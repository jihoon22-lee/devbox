# Devbox Knowledge

Hidden v0.8 B03 product integrating the existing Notes, Activity and Search engines.
The public v0.7 topology remains unchanged until cutover. PR #556 / WP #545 tracks
remaining migration, provider and runtime acceptance.

`pnpm --filter devbox-knowledge dev` opens the labelled browser fixture. On Windows,
`pnpm --filter devbox-knowledge tauri dev` runs the native product. Browser rendering
and portable tests are not Windows execution evidence.

The first native launch requires an explicit new-store action. A generation pointer
selects independent Notes, Activity and Search SQLite databases under the product's
installation-specific app-local root. Its private Markdown vault lives outside DB
generations. A corrupt/future pointer blocks initialization without resetting data.
No legacy source is initialized or imported by the new-user action. Existing-vault
selection remains blocked until the legacy-writer ownership gate is implemented.

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

Visited feature routes remain mounted to preserve drafts. Hidden modal handlers do
not trap another route's focus. The shared shell styles only its direct containers.
Notes defers Activity, Search and Mermaid; the build checks its actual static closure
against the preserved 970,000 raw / 325,000 gzip legacy ceiling and separately checks
the 280,000 / 90,000 outer-shell budget.

The Windows foundation fixture now includes explicit startup, private note writes,
default collection OFF, independent Search, replay/installation/role rejection and
mounted route retention in two installations. This fixture is registered but its
execution for host revision ceb2e7c passed. Additional Daily cancellation/save and
tray preference fixtures await Windows execution. Actual OS close/restart behavior,
legacy SQLite migration, vault quiesce, Activity summaries and source-aware opaque
Search references remain under implementation in this same B03 PR.
