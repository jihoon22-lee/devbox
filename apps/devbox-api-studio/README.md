# Devbox API Studio

v0.8 B02 API Studio. Product-internal functionality, migration and S03 have Windows acceptance; final installer coexistence and CI remain open. Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-api-studio dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-api-studio tauri dev` runs the native shell. Browser `?route=requests` selects a preview route. The Windows debug executable accepts `--route=requests` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.apistudio` identity; legacy data is accessed only by the explicit read-only importer. Navigation retains mounted route drafts in memory, with bounded history. B01 Windows installation identity evidence is recorded in the foundation workthrough. B02 native feature/runtime evidence is recorded in the workthrough.

Requests/Protocols, Webhooks and Transforms consume the shared frontend feature package and native adapters. See the [B02 workthrough](../../workthrough/2026-09-07-v08-b02-api-studio.md) for actual checks and remaining work.

Native dependencies disable the legacy crates' default `standalone` feature.
API Studio therefore embeds only its own frontend bundle. The three legacy apps
still build and run with their default features. Native OAuth uses the initialized
system-browser plugin through its validated API component; the renderer receives
no additional opener capability.

Internal Webhook history/fixture→Requests and selected response→Transforms use
this installation's `handoff/v1` namespace. Transfers open a preview and require
explicit apply. Raw webhook payloads and caller-supplied source records are not
accepted. A busy recipient preserves its pending action and the new publication
is revoked. Typed artifact provenance is metadata, not permission to read another
component's data. WP07 owns the remaining cross-product receiver delivery.

The native startup gate offers an import review before any feature mounts. It
reads the actual API browser keys, sealed environment/OAuth/TLS stores, Webhook
fixtures and service profiles, and Toolbox workflow metadata. API Playground must
be closed: the importer holds exclusive read handles on LevelDB files, copies
only that store, and runs a hidden exporter in an owned process on the copy.
The worker verifies its actual WebView2 data directory and cannot invoke product
commands. Raw temporary copies are removed after acquisition/export; a new
preview supersedes earlier plans and removes their staging directories.

The destination journal records source fingerprints and store-qualified IDs.
Conflicting IDs/names retain both records where the domain allows; existing OAuth
bindings and history capacity retain the destination policy. Completed receipts
make repeated imports preserve later product edits. Sealed values use the existing
API owner and DPAPI bindings; unavailable secrets become reconnect requirements.
Transient protocol sessions, raw header vaults, and HMAC inputs are excluded.

Applying the SQLite journal, native files and browser storage is an explicit
recoverable sequence. Startup remains blocked until native and browser values are
verified. Restart can continue a partial activation or restore its recorded prior
values; unexpected third-party edits stop restoration. A completed import cannot
be rolled back through the startup importer. No request, listener or process is
started by importing. `--import-legacy` or the in-app restart action opens another
review. Portable recovery tests and the actual Windows [migration fixture](../../.github/scripts/windows-api-migration.mjs)
pass on `bfc81b3`, including source preservation, DPAPI reuse and repeated imports.


History/Console previews HTTP requests and gRPC summaries separately from live
session payloads. Selecting a saved request preserves the current draft until
explicit replacement. Fixed protocol cancellation/expiry/reconnect errors retain
their original classifications across the product transport.

Closing the window stops its temporary Webhook listener and quits by default.
The Webhooks view can explicitly keep a running listener in the notification area;
its menu opens the window, stops the temporary listener or fully quits. Minimizing
or changing routes does not recreate/stop the listener. An unavailable tray falls
back to ordinary close. Service JSON exports stay disabled and never autostart.
Their `--service-profile <id>` entry point reuses the bounded native listener in a
separate process, without a Tauri App/event loop, WebView or API protocol bootstrap; its lifecycle belongs to the service runner.
The UI's listener and that process cannot bind the same port simultaneously.
Windows lifecycle acceptance is tracked by the
[lifecycle fixture](../../.github/scripts/windows-api-lifecycle.mjs).

The [transform command manifest](../api-studio-tools.json) lists the actual 21
tools and their output policies for the later WP07 provider. HMAC stays outside
handoff/pipeline publication; its existing explicit copy/file-save actions remain.
JWT decoded output is sensitive and passes native redaction. Unknown tool IDs,
renderer policy flags and invalid pipeline transitions cannot authorize exports.
Eligible tool/pipeline outputs can open a one-time Requests preview, with an
origin-form POST draft that requires explicit apply and a separately approved send.

Requests and Transforms can explicitly preserve a masked Knowledge draft in their
own `drafts/knowledge/v1/<owner>` directory. The UI accurately reports the receiver
as unavailable until WP07 supplies a verified connection. History/Console can
reopen, export or explicitly delete saved drafts after restart. Each owner keeps
at most 50 drafts with 512 KiB text bounds; full stores require explicit deletion.
Future/corrupt records and linked paths are preserved and rejected. Durable
artifact IDs do not expire with handoff leases and grant no cross-owner access.

Response bodies, eligible transform results and supported OpenAPI operations can
open a `mock-rule-draft/v1` preview in Webhooks. The recipient validates the HTTP
matching target, method, status and bounded masked body, with only a fixed content
type header. It never imports response credentials, file paths or execution flags.
Explicit apply replaces only the rule editor draft; rule storage and listener
startup remain separate actions. A busy preview is retained and each accepted or
cancelled publication is consumed once. Selected response text can also explicitly
replace either side of a Transforms comparison; those two inputs survive product
tool navigation without copying the text into metadata/history.

API Workspace keeps named, bounded ID sets for Collections, environments, saved
OpenAPI operation projections and persisted Mock service profiles. Selecting a
Workspace filters lists while preserving the current request/environment and
protocol connections. The API owner stores metadata with revisions, OS locks and
atomic writes. Deletes remove only the Workspace's links. Standalone workspaces
work immediately; Project association requires the current native-verified
ProjectId. WP07 must supply the actual Project provider before that option is
available. Renderer paths and arbitrary IDs never grant Project authority.

Explicitly saved OpenAPI projections preserve supported request templates and
Mock method/path/status, with the existing request type and native sanitizer.
Duplicate header order/enabled values and exact secret references survive; literal
credentials, including disabled headers and copied bearer tokens, are masked.
The API owner stores up to 32 definitions of at most 4 MiB each. These records are
not archives of the original OpenAPI document or unsupported schema semantics.
Reopening previews a masked draft; applying to Requests or publishing a Mock is
explicit and never sends or starts a listener. Full/future/corrupt stores preserve
their records; deletion requires confirmation and leaves Workspace references
visible as unavailable until the user edits them.

B02's 137 legacy command/UI/storage anchors and 10 preservation groups have explicit
implementation, test and importer mappings in the v0.8 inventories. Windows evidence
verifies 133 internal anchors and all 10 data groups; four receiver-dependent anchors
remain pending WP07. The
Webhook Logs producer exposes only the existing bounded `webhook-log/v1`
header-name/body-preview projection; raw headers never enter it. Until WP07
connects the verified Workspace Logs receiver, source actions report unavailable
and preserve the original request/fixture without launching a legacy executable.

The [S03 Windows fixture](../../.github/scripts/windows-api-workflow.mjs) exercises
capture→explicit request apply→native DPAPI credential reconnect→one explicit send→
masked response→native comparison→Mock editor→durable Knowledge fallback and restart.
It uses disposable synthetic profiles and a loopback server. Its registration is
separate from importer acceptance. The complete S03 scenario and listener lifecycle
passed alongside the importer and four native product probes on `bfc81b3` in
[Windows 34154505373](https://github.com/jihoon22-lee/devbox/actions/runs/34154505373).
That run still failed installer preflight because a synthetic native profile from
the migration fixture remained; its ownership-checked cleanup is under verification.

The build checks the complete static Requests import closure, including the shell,
against the preserved API Playground budget. OpenAPI/YAML parsing, Protocol Lab
and Transforms remain deferred. Parser-independent limits live in a separate
module so merely rendering Requests does not load the YAML engine.
