# v0.8 B07 — Global commands, search and product workflows

Refs #549, #541, #542. One B07 bundle covers Launcher/shortcut ownership,
federated providers, cross-product artifacts, Operations/Review and preference
migration. This branch starts from B06 02ba916 while its final acceptance runs.
Rebase only B07 commits onto merged B06 before publishing B07.

## Implemented first slice

The shared product-contract command descriptor names an owner/component, typed
route/entity reference, exact revision, context requirement, disabled reason and
review/destructive flags. Requests contain operation/command/revision/context and
an optional selection ID, never argv or content. Native owners retain authority;
a descriptor or preview is not execution approval.

Legacy Launcher now consumes the same ordered exact/prefix/substring field matcher.
Its field priorities, query normalization and bounded pre-cap ranking remain
unchanged. Control Center's bounded command index consumes Catalog v3's actual
feature command IDs and revision evidence, rejects duplicate provider identities,
and retains unavailable owners visibly. The new native command plugin authenticates
its local product session and exposes search/current-command preview only.

The Control Center products surface uses the native catalog, bounded debounced
queries and late-response rejection. A validated local route opens through the
existing shell navigation. Unconnected external providers remain unavailable;
this slice has not implemented cold/hot cross-product dispatch or global shortcuts.
Browser data stays explicitly labelled by the existing product-shell fixture mode.

## Native connection slice

All four products now consume the same application-platform pipe/installation
adapters and a lazy shared connection review surface. B07 defines a bounded
installation declaration with fixed suite/portable paths, exact version/digests
and no argv/environment/data-root fields. Native capture pins root ancestors,
manifest and executable objects against reparse/replacement; a declaration alone
starts no listener. Each running product requires an explicit, expiring review
of its own package before it accepts this generation's connections. Disconnect
revokes the capture and retires its listener/client tasks.

The Windows pipe owner rejects remote clients and duplicate registration, caps
connections/frames/deadlines and binds requests to a fresh native session. Both
ends identify the actual OS pipe process/image in the pinned installation;
blocking identity tasks own duplicated pipe handles so timeout cannot redirect a
reused handle. Foreign namespace/generation, unsupported protocol, stale session
and replay are rejected before owner dispatch. Only Control Center can currently
query/preview; other roles can describe their installed peer. The server exposes
its own catalog metadata and preview, with no cold launch or claimed navigation.

Catalog index logic moved into product-contract when the four product owners
became real consumers. Windows adapters remain in the app platform layer through
explicit source includes; affected CI records all four consumers. No new external
package version is introduced. The initial approval is process-local; B08 package
activation and durable reviewed installation selection remain separate work.

## Federated commands and received navigation

The Control Center now queries each connected product's command catalog separately.
Per-source deadline/errors and late-generation rejection preserve other results;
foreign same-name commands keep owner-scoped IDs. Preview resolves the current
remote descriptor. A destination-owned, bounded navigation queue separates
awaiting review, opening, opened, rejected and expired. Exact operation/content
receipts prevent duplicate opening; an ID reused with different content is rejected.

Each product's shell receives a notification and lets the user open or reject the
requested route without replacing project context or discarding a mounted draft.
The receiver acknowledges only the selected route after UI navigation. The sender
retains an unknown delivery receipt when a reply fails, allowing an explicit status
check without automatically repeating the command. Entity/context navigation still
requires its actual domain adapter and is explicitly unavailable in this slice.
Pure queue fixtures cover duplicate/conflict/reject/expiry/revocation and incorrect
acknowledgement; a UI fixture covers a slow source and stale-generation response.

## Reused Launcher and shortcut owner

The actual legacy Launcher UI now lives under product-shell/launcher with an
injected adapter. The legacy entry supplies its unchanged native API/clipboard
behavior and retains its existing UI tests. Scoped CSS prevents palette styles
from changing other product screens. Control Center consumes the same keyboard,
IME, selection, stale-review and favorite controls in a lazy modal while retaining
its current route. Product queries stream independently through the adapter;
unsupported owners remain visible and cannot be opened accidentally.

The existing ID-only Preferences implementation and its tests moved to
product-contract for the second native consumer. Control Center stores favorites
and local route recents in its own installation namespace, requires current
remote metadata before adding a favorite, and permits removing an unavailable
favorite without reopening its provider. Corrupt settings are not overwritten.

The existing Windows hotkey worker now accepts a bounded binding list and callback
while its legacy entry preserves the same three allowed Launcher accelerators.
Control Center uses one worker and an OS existence lease so another v0.8 install
cannot simultaneously own global shortcuts. Configuration requires an approved
package connection, attempts previous-key restoration after registration failure,
reports restoration/foreign-owner failures, and retires on disconnect/exit. It
adds no startup entry. The initial product binding opens the actual Launcher;
Terminal/Capture/current-project bindings remain rejected until their domain
handlers are connected. No Ctrl+C binding is registered. The palette checks active
modal/IME/editing state and distinguishes an already-focused host from an external
activation before changing focus.

## Verification and remaining work

Regression fixtures are authored for stale/disabled/forged references, required
review/context/selection, inherited ranking, duplicate commands and late UI queries.
Rust syntax parsing/formatting and the Control Center TypeScript check passed in
1.813 seconds under the shared resource wrapper after the first slice was wired.
The connection slice also passed Rust syntax parsing and the Control Center/shared-shell
TypeScript check in 2.878 seconds. The next navigation/federation slice passed the
same minimum TypeScript check after correcting an unused test import; Rust files
were syntax-parsed/formatted. Its regression fixtures have not run yet. The shared Launcher/host passed the two
consumer TypeScript check in 4.484 seconds; Rust changes were syntax-parsed. A
missing direct Tauri dependency was resolved through the existing shared adapter
boundary, without adding a package version. Subsequent presentation changes were
checked with the same minimum typecheck, not tests/builds. No B07 Cargo compilation, tests, build, Clippy or affected run has occurred. Detailed
verification waits until the whole B07 bundle is implemented. Node dependencies
were installed from the existing offline lock/store; no package versions changed.

Next: cold launch and durable approved installation selection; one shortcut owner and diagnostic settings;
federated metadata/content providers; source-owned artifact claim/restore/ack and
Knowledge summary/capture; shared operation/review projections; legacy preference
mapping; native end-to-end fixtures. B08 owns final installer/activation topology.
