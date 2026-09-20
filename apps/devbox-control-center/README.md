# Devbox Control Center

v0.8 Control Center under development (B07 command host and B08 installation/recovery). Hidden from the v0.7 release and Manager catalog.

`pnpm --filter devbox-control-center dev` opens the explicitly labelled browser fixture. On Windows, `pnpm --filter devbox-control-center tauri dev` runs the native shell. Browser `?route=products` selects a preview route. The Windows debug executable accepts `--route=products` and validates it against this product’s registered routes before creating its webview. Native requests are restricted to the local main webview and validated by `product-shell-tauri`; route selection does not grant domain authority.

Uses a new `com.devbox.v08.controlcenter` identity and an installation-specific namespace. Manager diagnostics read the original Manager root explicitly; new product writes remain in the new namespace. Navigation retains mounted route drafts in memory, with bounded history. Native runtime and installer identity proof must be recorded in B01 acceptance before claiming Windows parity.

B07 connects the actual Launcher, bounded native search providers, four managed
shortcuts, explicit artifact reviews and shared Operations to the four products.
Package approval pins the installation's manifest and executable identities;
remembered cold activation and native peer checks retain that exact namespace.
Launcher preferences migrate by exact opaque IDs with unresolved entries preserved.
Operations reports owner state and opens owner reviews without acquiring execution
or cancellation authority. The native four-product fixture and one PR audit own
acceptance; implementation commits alone are not acceptance evidence. See the
[B07 workthrough](../../workthrough/2026-09-13-v08-b07-global-commands.md).

B08 embeds Manager Environment, Data Inspector, diagnostics and reviewed package-only
Related Tools. Products/Components distinguish verified package files, absent members,
and unknown runtime or installer state. Legacy ARP observations use the pinned v0.7
**installed-file** reference; public portable executables have different bytes.
Inventory alone does not authorize cleanup, and the old uninstaller is never invoked
against a user's installation to bypass exact ownership checks.

The private `devbox-suite-bootstrap` currently implements these Windows operations:

| Operation | Result |
|---|---|
| `--stage ROOT PAYLOAD` | Verify exact archives and stage a newly owned product directory |
| `--prepare-install ROOT PAYLOAD` | Prepare an isolated generation, retained journal and Import marker |
| `--register-install ROOT PAYLOAD` | Register one current-user Suite entry and four owned Start Menu links after NSIS creates its uninstaller |
| `--uninstall-install ROOT PAYLOAD` | Remove verified package files, shortcuts and the Suite registration; preserve user stores, backups and unknown files |
| `--recover-install ROOT PAYLOAD` | Block and recover an uncommitted first installation while preserving all data and packages |
| `--restart-install ROOT PAYLOAD` | Start a fresh package generation after that recovery; preserve the old journal and files |
| `--verify-checkpoints ROOT PAYLOAD` | Verify journal-selected checkpoint identity, exact product file sets and all retained bytes without restoring them |
| `--snapshot-install ROOT PAYLOAD` | Preserve the four closed product namespaces, including DB/WAL and closed browser files, without changing activation |
| `--prepare-data-restore ROOT PAYLOAD CHECKPOINT_ID` | Preserve current data and prepare an installation-bound restoration of a recorded checkpoint |
| `--apply-data-restore ROOT PAYLOAD OPERATION_ID` | Apply or resume the reviewed directory swaps, then require fresh four-product health |
| `--commit-data-restore ROOT PAYLOAD OPERATION_ID` | Commit the restored data after fresh native health; ordinary writers remain blocked until this step |
| `--rollback-data-restore ROOT PAYLOAD OPERATION_ID` | Undo an uncommitted restore while retaining original and restored/reviewed data |
| `--prepare-update ROOT NEW_PAYLOAD` | Stage a complete new generation and preserve the closed current data |
| `--prepare-and-apply-update ROOT NEW_PAYLOAD` | Prepare/apply an update, or resume its existing recorded operation |
| `--apply-update ROOT NEW_PAYLOAD OPERATION_ID` | Resume owned data-directory swaps and publish the new generation in Health mode |
| `--commit-update ROOT NEW_PAYLOAD OPERATION_ID` | Commit after four fresh product health reports and update Suite registration |
| `--rollback-update ROOT NEW_PAYLOAD OPERATION_ID` | Restore the previous package records and original data directories before commit |
| `--open-install ROOT PAYLOAD` | Launch the pinned Control Center and derive its setup/recovery view from the native activation marker |

`PAYLOAD` is the private `suite-payload.json` from `build-suite-package.py`, with its
four verified ZIPs beside it. The bootstrap verifies its own payload identity first.
Recovery/restart require all product writers to have closed; they do not terminate
processes by name. A committed installation or an update with a previous generation
requires a separate data recovery plan and is rejected by the first-install commands.
Product data checkpoints require the exclusive installation writer gate and retain
all original files. They are stored in the installation's separate local backup
namespace and recorded independently from legacy source backups. No external vault,
Git directory, legacy namespace or source schema is copied through a referenced
path. Restoration is an explicit separate operation within the recorded package
generation; it does not permit a package/schema downgrade. Its journal lives outside
all four stores. Physical directory identities permit restart after each rename,
and a durable startup gate excludes browser/window-state writers during partial
swaps. Original directories and changes made during health review are retained when
undoing an uncommitted restore. Retention is bounded and never silently deletes old
copies. The Recovery screen selects recorded checkpoints and explicitly hands
reviewed actions to the retained helper. Control Center closes after handoff;
the helper waits for all product leases without killing processes, then reopens
Control Center. Preparation and apply run together without reopening a browser
between the captured preimage and its replacement. After an interrupted swap,
Control Center's normal entrypoint resumes the helper before creating a WebView.
Windows restore/undo acceptance passed in the B08 installed fixture; see the workthrough for scoped sources.
The private `build-suite-installer.py` wraps preparation in a Windows NSIS entrypoint,
creates its uninstaller and delegates Suite registration and four product shortcuts
to the verified helper. Its finish page reports preparation, not activation completion.
Removal runs the embedded helper outside the package tree, checks every planned
file's digest and physical identity, and can resume a partially removed package.
Unknown files, user stores and backups remain intact; small removal/ownership records
remain available for recovery. It never invokes a legacy uninstaller or recursively
deletes a user-selected directory. Windows installer/uninstall acceptance passed in the B08 installed fixtures. Public release remains B09. The update path keeps the
original package generation and physical data directories, runs the new version on
copied data, and blocks ordinary writes until native health and explicit commit.
A durable startup blocker covers partial metadata publication. Existing shortcuts
remain pinned dispatchers that resolve the current verified helper; the original
NSIS uninstaller delegates to a verified temporary copy of the current helper.
An older suite version is rejected with a backup/export requirement, rather than
opening newer stores in an older executable.

Product migration summaries report setup/review/busy state through authenticated
native owners. They do not authorize activation. Import mode admits only the closed
owner importer/review methods and keeps schedules and ordinary business writes off.
All four renderers defer business views until commit, including browser-storage
writers. Knowledge can prepare stores without starting engines; Health/Recovery
allow bounded native observations but cannot invoke owner importers.
Updates and Recovery can read fresh native package/shell/store observations from
all four approved product owners. The probe never launches products or user work;
pending imports remain unready. Renderer parity, source backup and activation
commit are separate gates.
Migration can record each owner’s authenticated summary and verified retained
backups into the suite journal. A changing session/catalog or failed backup leaves
the journal unchanged. These retained observations do not authorize cutover.
Source-aware cutover now binds fresh owner observations to an explicit review,
then acquires closed-source handles before activation. Installed update/recovery,
shortcuts/ARP/uninstall and reviewed legacy cleanup passed the B08 Windows fixtures. See the [B08 workthrough](../../workthrough/2026-09-13-v08-b08-suite-recovery.md)
for actual evidence and the final CI merge gate.

Local tests must preserve existing services and host networking. Provisioning and
network-changing Windows/WSL/Docker fixtures run only on disposable hosted VMs under
[verification operations](../../docs/verification.md). Per-commit checks remain
minimal; detailed acceptance runs after the entire PR bundle is implemented.

Launcher migration preserves exact original preference/shortcut bytes before
changing its destination, and retains earlier completed plans with their exact ID
mappings. Resume checks the retained digest; an older journal may acquire the new
backup only from a still-identical legacy source. These private records are not
included in diagnostics. Their bounded retention never silently deletes an older
completed backup or plan.

Migration lists each connected owner's retained backups and offers explicit content
verification. Owners resolve opaque IDs inside their own namespaces; source paths
and record contents are not sent to Control Center. Original SQLite/browser/JSON
backups and normalized import recovery snapshots are labelled separately. A
verified retained backup does not prove that its legacy source has stayed unchanged
or that all sources were imported; cutover revalidation remains a separate gate.

For a clean first install with no legacy data namespaces or imported records,
`--activate-clean-install ROOT PAYLOAD` requires the four recorded owner results,
preserves closed product data and enters Health. Reopen the products and record
fresh health in Control Center, close them, then use `--commit-clean-install`
with the same arguments. Reports expire after five minutes. The helper rechecks
package ownership and preserves another closed checkpoint before enabling writes.
The installation/recovery screen now exposes these explicitly reviewed clean-install
actions and only offers them when native observations indicate no legacy sources.
These commands passed private installed acceptance; exact-main public release remains B09. Existing
legacy data uses the source-aware review below; installed updates use the copied-data
generation coordinator.

The Updates screen checks the official stable release only on request. Native code
validates the seven public assets, streams the reviewed setup into a bounded cache,
checks its size/SHA-256, and supports cancellation. Opening the installer requires a
separate confirmation. Existing data/package generations remain recoverable until
fresh native health and commit; downgrade is blocked with a backup/export requirement.

Source-aware first activation records fresh original-source comparisons with each
owner observation. SQLite comparisons use consistent online snapshots; closed
browser/native sources use their accepted retained bindings. Knowledge follows the
selected generation ancestry. Raw namespace fingerprints bracket the observations
and include WAL while excluding transient SQLite shared-memory files. External
paths found in preferences are never followed. Each present source needs an explicit
choice: use reviewed import results whose originals still match, or preserve the
original as skipped. A retained backup is not a complete-coverage claim. The helper
requires legacy/product processes closed, retains original source handles, and
rechecks inventory before publishing Health/Committed. Changed sources can return
to Import while preserving current product data and requiring new observations.

After commit, legacy cleanup exposes separate preview/confirm/resume controls.
Windows installer cleanup uses the pinned v0.7 installed-file reference, exact ARP
metadata and verified shortcuts. It never executes registry uninstall strings.
Manager-owned portable cleanup retains its reviewed file identities and Manager
manifest/location CAS, including interruption after the manifest claim. Unknown,
changed or inaccessible files remain preserved; cleanupPending stays separate from
successful Suite activation. No privilege escalation is performed.

A completed data-preserving uninstall retains ownership/removal records. Reinstall
with the exact last package restores the same installation/data identity, preserves
a checkpoint and requires fresh four-product health before restoring ordinary use.
A different package is blocked: recover the original package first, then use the
normal generation update path. Reinstalling an uncommitted first installation
returns to Import review. Durable intents support interrupted package staging;
old removal records are retained instead of reused to delete newly restored files.

Interrupted first setup now resumes as setup, rather than being misclassified as an
installed update. Before retrying incomplete extraction the helper retains its
partial package tree; a completed generation with changed files still fails closed.
Space preflight counts package/copy requirements and checks Windows free space.
Failures retain original data and show a specific space/closure recovery message.
Postcommit cleanup does not republish a global legacy Manager root/catalog.

Products includes an explicit installation-folder action. Native code captures and pins this executable’s package directory before opening Explorer; it accepts no renderer-provided path. Unknown installation state keeps the action disabled.
