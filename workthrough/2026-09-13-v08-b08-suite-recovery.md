# B08 — Control Center and recoverable suite delivery

Refs #550, #541, #542. B08 is one implementation/importer/installer/fixture PR.
Base is the in-progress B07 integration (f902759); rebase onto its accepted main
before final PR verification. B07 and B06 verification evidence is tracked in their
own workthroughs. No production installation or migration is authorized by a test.

## Implementation boundary

Reuse Manager's existing doctor, Data Inspector, redacted support bundle, Related
Tools and package-only Dev Setup. Keep its legacy batch installer separate from
new generation activation. Expose only four user products. Inventory must distinguish
unknown installer evidence from absence and identify exact Manager-owned portable
registrations; arbitrary uninstall strings and guessed executable paths are denied.

Compose the existing owner importers with consistent WAL-aware source acquisition,
explicit source/destination reviews and resumable receipts. Package stages preserve
the previous generation; quiesce, activation, health and commit are explicit phases.
A verified bootstrap helper owns Control Center replacement. Post-commit downgrade
and uninstall preserve new user data and external vault/repository edits.

The v0.8 asset goal is one suite setup, four product ZIPs, manifest and notices;
its actual manifest/assembly/installer contract must be implemented and verified
before B09 uses it. No public RC or unverified stable promotion.

## Verification policy

Commit checks are minimum syntax/types and diff/plan review. Detailed verification
runs once after all B08 implementation/importers/fixtures are assembled. Add only
missing native installer and fault-injection acceptance, retain unrelated passes.
All network/service-changing fixtures run on disposable hosted Windows VMs. Never
provision local Docker/WSL or mutate existing services, firewall rules or routes.


## Existing Manager tools as an actual second consumer

Manager UI/tests moved to control-center-features with legacy wrappers. Control
Center consumes its controlled tools modes without legacy catalog/install refresh
or install AppLink handling. Styles are scoped to the shared tools root; standalone
Manager retains its document reset. Existing native doctor/diagnostics/support,
Related Tools and reviewed package-only Dev Setup commands are embedded through a
local Control Center route allowlist. No legacy batch install or bootstrap runs.
Official URLs resolve against the native curated list without installation probes.
The new tool component/capability and catalog dependency edges are registered.
Dependency locks add only the internal consumers; unrelated transitive pnpm changes
from lockfile resolution were removed. No B08 detailed test/build/audit has run.


## Inventory and recovery implementation

Native package capture supplies a four-product/component inventory without starting
peers. Unknown binary/ARP/runtime observations stay unknown; a verified portable
manifest can separately report an omitted product. Products/Components consume the
local delivery authority. Recovery displays durable journal metadata only; it does
not repair/delete user stores. Manager diagnostics remains a separate existing tool.

The generation journal records stage/verify/backup/import/validate/quiesce/activate/
health/commit/cleanup phases, owner/source/destination mappings and retained backup
metadata. Conflicting replay, stale revisions and incomplete cleanup are explicit.
Precommit recovery blocks writers before restoring prior packages; postcommit cleanup
failure keeps the new suite active. Native durable storage uses a retained lock,
filesystem identities, bounded JSON and compare/write revisions. The actual package
stager/bootstrapper/owner importer orchestration is not connected yet.

Product namespace selection now recognizes only verified closed generation layouts.
The physical root identity plus manifest installation ID keeps data identity across
package generations; a copied manifest ID in another root does not alias that data.
Direct portable/development location identity remains compatible. The executable
hash/version and active generation must match before selecting a generation's data
namespace. Synthetic regression cases are prepared, not executed ahead of B08's
complete PR audit. Installer startup/write barriers and health/commit integration
still need implementation before this is an accepted activation path.


The package contract now requires one setup, four product ZIPs, notices and a
schema-2 release manifest. A deterministic assembly script binds the source SHA,
exact file closures and standalone portable declarations; it does not build or
publish anything. The stager validates archive and member hashes/limits, rejects
unlisted paths, links, encrypted/directory entries and existing destinations, pins
Windows stage directories, and suppresses the portable manifest that would shadow
a suite root declaration. Synthetic escape/cancellation/preservation fixtures are
prepared for the final B08 audit. No package staging has run on a user installation.

A shared product/worker lifetime lock now excludes late v0.8 writers while the
updater holds the exclusive gate. The API service and browser preservation workers
retain the same lease. Generation installations fail closed if their gate is
missing or an update owns it. Detailed bootstrap/legacy writer quiesce and controlled
health/import worker admission remain to connect; this is not completed recovery
acceptance. Minimal library type checks passed (34.755 seconds), with unused legacy
Manager entrypoint warnings in embedded mode to resolve before final Clippy.


Legacy inventory now reads Manager's original identifier namespace and pinned
v0.7 catalog revision instead of treating the Control Center data directory as a
legacy root. ARP discovery reads bounded current-user/machine x64/x86 registry
views and reports partial access and uninspected other users explicitly. ARP strings
are retained only as hashed observation identities; no uninstall command is run.
Manager-owned records and Windows installer declarations are separate in Migration.
Actual executable/uninstaller proofs and reviewed cleanup are still to implement.
The embedded library excludes the unused standalone AppLink module; legacy Manager
install entrypoints remain unregistered, with their intentional dead code annotation
limited to that module when standalone is disabled. ZIP's full existing deflate
feature now selects a backend even when Control Center is compiled alone. Minimum
native/frontend type checks passed after that feature correction (9.620 seconds).


## Bootstrap and activation barrier

Control Center now has a separate devbox-suite-bootstrap binary target. Its stage
entrypoint validates its own image against the private payload, reuses Manager's
local-root protections, admits a new empty generation directory, and records a
root-identity/payload-bound owner and receipt. A fully written interrupted stage can
be re-observed without overwriting files; partial or unrecognized content requires
recovery and remains preserved. No registry edit, active-generation replacement or
user-data migration is implemented by this stage-only command.

The shared native activation marker distinguishes Import/Health/Committed/Recover.
Generation product requests cannot start business writers before commit; native
Runtime initialization, Knowledge background activation and standalone API service
workers check the same barrier. Peer command execution also checks it. Products
show the pending phase in their shared shell. Direct development/portable behavior
is preserved. The bootstrap install/finalize/rollback orchestration, owned-worker
admission, owner importer reports and installer UI remain unfinished.

Minimum library/bin type checking passed after using the existing opaque operation
ID contract (37.819 seconds). The completed bootstrap/native description and shared
shell types passed (7.497 seconds). No B08 detailed tests, builds, actual staging or
installer execution have run. The ARP module now belongs only to Control Center,
not the shared suite platform source included by all four product crates.


Legacy binary observation now verifies declared installer executables against the
actual public v0.7 release manifest, pinned to the existing baseline SHA-256. The
manifest was downloaded read-only and matched the 15-app baseline before inclusion.
Native ARP inspection validates the declared local root, exact display-icon target,
version, executable size/hash and file identity. This confirms only the executable;
uninstaller verification/cleanup remains separate. Minimum CC library/bin and UI
types passed (8.071 seconds); no installed legacy app or uninstaller was run.


The pinned v0.7 lock uses Tauri CLI 2.11.4. Its upstream NSIS template calls a
basename-based process-kill helper during uninstall and removes a shared autostart
value. Therefore B08 must not execute that uninstaller against live user installs;
cleanup will instead delete only verified files/registrations/shortcut targets.
Source: https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh
and the adjacent installer.nsi uninstall section.

A manual-only hosted reference acquisition is prepared to derive the immutable file
closure (including uninstall.exe) from each pinned public installer at two owned
roots. This resolves the concrete missing ownership reference for safe cleanup;
it builds no product and starts no legacy app. No local execution is permitted.
Detailed B08 fault/installer acceptance remains at PR completion.

### 2026-09-19 재개: 첫 설치 준비와 owner 관측 계약

- bootstrap `--prepare-install`은 root/실행 파일/payload identity를 확인하고 generation,
  namespace, writer lock, journal과 Import marker를 준비한다. 일반 실행의 쓰기 권한은
  여전히 닫혀 있으며 이전 완료나 활성화 성공으로 보고하지 않는다.
- owner의 제한된 importer와 제품 연결 검토만 Import 단계에서 허용한다. Workspace는
  importer 저장소만 열고 scheduler/maintenance를 시작하지 않는다. 세 owner의 native
  migration summary를 Control Center에서 명시적으로 조회한다. 이는 commit proof가 아니다.
- 이전 최소 확인 결과 보존: install preparation Rust check 2.831s, owner coordination
  Rust/TypeScript check 25.806s PASS. full PR 검증은 아직 실행하지 않았다.
- pinned legacy installer의 installed-file closure 수집은 기존 foundation workflow의
  수동 `legacy_reference_only` 모드로 분리했다. 일회성 hosted Windows VM에서만 실행하고
  제품 재빌드·Docker·로컬 설치는 하지 않는다. 실제 사용자 설치에는 legacy uninstaller를
  실행하지 않는다. 수집 결과가 확보된 뒤 exact-file cleanup을 구현한다.

### 실제 legacy 설치 identity 및 첫 설치 취소

- [hosted reference acquisition 35431855772](https://github.com/jihoon22-lee/devbox/actions/runs/35431855772)
  PASS: pinned v0.7 installer 15개를 두 owned root에 각각 설치하고 완전한 파일 목록을 비교했다.
  실행 파일·uninstaller·notices 및 Code Pad companion의 SHA/size가 두 경로에서 동일했다.
  앱은 실행하지 않았고 fixture 소유 설치를 제거했다. 수집에 제품 build는 없었다.
- 모든 앱의 installed executable은 public portable executable과 hash가 달랐다.
  ARP inventory는 portable SHA 대신 검증된 installed reference를 사용하도록 수정했다.
  reference 원본 provenance는 `resources/legacy-v0.7-installed-files.json`에 보존한다.
  현재 검증은 소유 파일 확인이며 사용자 설치 제거/정리 실행은 아직 연결하지 않았다.
- `--recover-install`은 미확정 첫 설치만 처리한다. native root/payload/journal identity를
  대조하고 exclusive writer gate를 잡아 모든 제품이 종료됐음을 요구한다. Recover marker를
  먼저 기록한 다음 journal을 종료하며 파일/legacy 원본/이전한 v0.8 데이터는 삭제하지 않는다.
  committed 설치나 previous generation이 있으면 별도 데이터 복구 계획을 요구한다.
- journal은 다음 작업을 시작할 때 완료된 기록을 보존한다. 미완료 작업 교체, 설치 identity
  변경, 실제 이전 generation과 다른 연결을 거부하며 archive 이후 crash 재개 fixture를 준비했다.
- 최소 Rust check: 설치 reference 4.998s, bootstrap recovery 2.131s PASS.
  앞선 non-Windows branch의 unit/Result 혼합 타입 오류를 수정했다. 새 regression fixture는
  PR 구현 완료 시 실행한다. B08 전체 검증/실제 Suite installer acceptance는 아직 미실행이다.

### 중단한 첫 설치 재시작

- `--restart-install`은 이전 작업이 Recovered이고 활성화된 이전 세대가 없는 경우에만
  새 operation/generation을 할당한다. 기존 부분 파일과 가져온 데이터는 보존하며 새 package
  slot을 준비한다. 다른 payload나 이미 committed인 설치에는 이 경로를 쓰지 않는다.
- restart intent를 먼저 보존하고 completed journal을 archive한 뒤 새 journal을 시작한다.
  intent 기록 직후 crash는 `--prepare-install`로 이어갈 수 있다. 이전 manifest 교체는
  archived Recovered journal과 Recover marker가 모두 일치할 때만 허용한다.
- 최소 Rust check 1.829s PASS. archive read-back/동일 slot 거부/사용자 데이터 보존 회귀
  fixture를 준비했으며 PR 완료 시 실행한다. 실제 setup/업데이트/활성화 수용은 아직 남아 있다.

### Suite setup 준비 진입점

- `build-suite-installer.py`는 기존 네 ZIP/private payload와 일치하는 bootstrap을 확인하고
  Windows NSIS 준비 진입점을 생성한다. `--emit-only`로 source를 검토할 수 있으며 실제
  compile은 Windows의 명시적 NSIS 경로를 요구한다. 현재 public release에는 연결하지 않는다.
  [NSIS Modern UI](https://nsis.sourceforge.io/Docs/Modern%20UI%202/Readme.html)의 finish-run
  function으로 Control Center 열기를 선택한다. 화면은 활성화 완료가 아니라 준비 완료라고 표시한다.
- 설치 폴더의 첫 생성도 helper가 수행한다. 기존 plain ancestor와 prospective root를 먼저
  확인하고 native directory identity를 유지한다. NSIS가 검증 전 UNC/junction 폴더를 만들지 않는다.
- `--open-install`은 root/payload/manifest/marker를 대조하고 pinned member만 실행한다.
  WebView override와 stdio를 전달하지 않아 다른 제품의 프로필이나 NSIS pipe를 물려주지 않는다.
  제품의 `--suite-setup`은 release에서도 native marker로 migration/recovery/products 화면을
  선택한다. 일반 `--route` 개발 경계나 command 권한은 확대하지 않는다.
- Python AST/diff 및 최소 Rust check PASS (`b08-setup-final-types.log`). 실제 NSIS compile,
  helper launch, ARP/shortcuts/uninstall/update와 health/commit은 B08 전체 수용에 남아 있다.

- Setup assembler의 tampered helper/ZIP, manifest 경로 이탈, mixed version,
  기존 candidate 보존 fixture를 준비하고 dependency policy CI에 등록했다.
  Python AST/diff만 확인했으며 전체 구현 완료 전 반복 실행하지 않았다.

### Owner 이전 매핑 관측 연결

- API Studio는 browser/native 적용을 모두 승인한 complete import의 retained mapping만
  집계한다. Knowledge는 선택된 세 store의 실제 ID receipt를 읽는다. record ID·경로·본문은
  공유하지 않고 bounded count와 deterministic revision만 Control Center에 반환한다.
- Knowledge 각 DB는 독립 read transaction이며 global transaction으로 주장하지 않는다.
  신규 source backup이나 활성화 완료 proof로도 취급하지 않는다. Workspace의 미연결 매핑
  관측은 0으로 표시하지 않고 확인 필요로 남긴다.
- blocking owner reader는 제품당 한 슬롯으로 제한하고 async pipe/event loop에서 분리했다.
  UI의 명시적 상태 읽기에만 실행하며 background polling을 추가하지 않았다.
- 최소 Rust/Control Center TypeScript check PASS 46.395s. 브라우저 미승인 행 제외,
  opaque ID·destination 변경·잘못된 receipt 타입 회귀를 준비했으며 PR 완료 때 실행한다.

### Native product health observation (2026-09-19)

- Added a Control Center-only health call over the already approved physical
  installation bus. Native challenges, generation, installation key, live main
  shell session, catalog revision and exact routes are checked at the receiver
  and host. No cold launch, business operation, service or scheduler is invoked.
- Owners read their existing migration/store state on bounded blocking workers.
  Workspace additionally revalidates selected component directories and Registry;
  Control Center reads Launcher preferences, shortcut config and import journal
  without applying settings or registering shortcuts. Incomplete imports remain
  unready. Responses contain digests/state, not raw preferences or user records.
- Updates and Recovery show the four independent observations. These are native
  package/shell/store observations, not renderer parity, a global data transaction,
  source backup evidence or an activation commit permission.
- Prepared contract regressions for stale challenge/generation/owner, repeated
  routes, pending/busy import and non-Control-Center callers. Detailed tests stay
  scheduled for the completed B08 bundle. Minimal Rust checks for the four owners
  and Control Center TypeScript passed in 33.722s under the shared resource budget;
  Windows-only adapter execution remains pending.

### Closed product-data checkpoints (2026-09-19)

- `--snapshot-install ROOT PAYLOAD` verifies the native helper/payload, physical
  installation owner, manifest, activation marker and journal, then acquires the
  exclusive suite writer gate. Any running product causes refusal; no process is
  terminated. Native identifiers derive exactly four installation-specific local
  data roots. No renderer supplies a source/destination path.
- Copies complete closed namespaces, including SQLite DB/WAL and WebView files,
  into a separate private backup namespace. Retained directory identities,
  no-link admission, bounded sizes/time/file counts, stable source re-reads and
  copied-file digests precede publication of the complete manifest. This is a
  quiesced copy, not a live `.db` copy or global transaction with external stores.
- External vaults/repos referenced by settings and all legacy namespaces remain
  outside this operation. A failed copy is retained without a completion marker;
  a complete copy survives journal-write failure. The journal separately records
  product checkpoints and legacy source backups. At 32 recorded checkpoints the
  helper requires retention review, never automatic deletion.
- Added synthetic WAL-only row, closed browser bytes, absent-product, cancellation
  and outside-link regression fixtures. They are prepared, not executed yet.
  Initial typecheck found SHA formatting/test-helper errors; those were fixed.
  Scoped Rust (including test compilation) and Control Center TypeScript passed
  in 6.237s. No detailed test/build rerun. Windows acquisition and reviewed data
  restoration remain pending; this does not complete update/downgrade acceptance.

- Checkpoint continuation: `--verify-checkpoints` reads only IDs/digests from the
  native installation journal and verifies each complete private manifest, exact
  per-product directories/files, absent owners, byte counts and file hashes.
  Wrong installation/generation, added/missing/changed files or malformed records
  fail closed. No restoration is performed. The WAL fixture now validates the
  checkpoint and opens a separate copy, leaving the accepted checkpoint untouched;
  it also covers wrong-owner/generation and changed browser bytes. Scoped Rust
  and test compilation passed in 1.568s; prepared tests still await B08 completion.

### Retain API Studio source backups through later imports (2026-09-19)

- Closed browser acquisition now creates a separately verified `retained-leveldb`
  copy before the worker WebView can open/compact its `webview-copy`. The original
  source remains read-only. The existing closed-source receipt describes retained
  bytes; cleanup only removes the worker copy/ticket and never the retained copy.
- The next import archives completed stages under `imports/retained/<id>` instead
  of deleting them. Normalized snapshots, reviewed plans and source backup metadata
  survive alongside the existing completed ID mapping journal. Unaccepted previews
  retain their supersession behavior. At 32 retained operations, imports require
  retention review; there is no automatic completed-backup deletion.
- Prepared shared-copy tests for worker mutation, retained-byte tampering and bad
  names, plus API regressions for completed-stage retention and worker cleanup.
  API/shared migration Rust and test compilation passed in 12.935s. Detailed
  tests and Windows acquisition remain scheduled for the completed B08 bundle.

### Launcher source and completed-plan retention (2026-09-19)

- Preserve the exact legacy preference/shortcut bytes (including absent-file
  state) in a content-addressed private record before any destination change.
  Source acquisition now re-reads both files as a pair; this is a stable JSON
  observation, not a global transaction. Apply still compares the fresh source
  revision and native exact mappings against the review.
- Resume verifies the retained source digest. For an older journal without the
  new backup, only an exact hash match to the unchanged legacy source can create
  it; changed originals block resume rather than fabricating backup provenance.
- Starting another accepted import archives the prior committed plan, including
  exact ID mappings. Existing backup/history bytes are never overwritten after
  tampering; bounded retention requires review instead of silently deleting them.
- Prepared idempotence, source tampering and path-rejection regression. Scoped
  Rust and test compilation passed in 2.363s; runtime tests remain deferred to
  the complete B08 bundle.

### Four-owner retained backup observation (2026-09-19)

- Added bounded Control Center-only listing/verification calls over the approved
  native installation connection. Inputs are opaque IDs; each owner resolves its
  own private paths. Foreign callers and path-shaped IDs are rejected. Responses
  contain owner, acquisition type, schema, size and digest, never source paths or
  record contents. Ordinary writers remain blocked during Import/Health.
- API Studio verifies retained browser bytes against closed-source receipts and
  normalized import SQLite bundles against the completed native activation
  journal. Knowledge verifies source SQLite snapshots recorded by activated plans.
  Workspace verifies its compiled-in JSON snapshot inventory. Control Center
  verifies retained original Launcher byte pairs and their accepted schemas.
- Migration UI distinguishes original backups from normalized import recovery
  snapshots. Listing is separate from explicit per-backup verification; unknown
  owners, unavailable bytes and failed checks stay visible. These observations
  do not certify complete source coverage, fresh legacy source state or activation.
- Added protocol-role/path regression and extended the existing API archive and
  Knowledge activated-plan fixtures with backup verification/tamper checks.
  Four-owner Rust/test compilation passed in 25.070s, UI types in 3.133s. Detailed
  regressions/Windows IPC execution remain pending with the completed B08 bundle.

### Bind retained native source bytes to API activation receipts (2026-09-19)

- Native source acquisition now copies only compiled legacy JSON paths and
  reviewed profile IDs into the import's private `retained-native` directory.
  Missing files are explicit; profile-list/source-byte changes between passes
  reject capture. File/count/aggregate/time/cancellation bounds apply. Parsing
  and DPAPI resealing read the preserved bytes, never a second unbound source.
- Completed activation bundles retain optional native/browser backup metadata
  digests. Backup catalog uses those accepted fields, preserving missing expected
  backups as verification failures. Verification binds the private metadata hash
  to the accepted journal as well as checking retained file bytes. Older bundle
  schema reads remain compatible; absent bindings are not invented.
- Added opaque-ciphertext/original preservation and accepted-manifest tampering
  fixtures. Initial typecheck caught missing Bundle initializer fields; corrected
  scoped API Rust/test compilation passed in 4.948s. Prepared regressions/native
  importer execution stay scheduled for complete B08 verification.

### Keep precommit products in migration/readiness mode (2026-09-19)

- Separate native installation review from owner importer admission. Health and
  recovery may read bounded metadata; only Import admits migration writes.
  Ordinary commands and schedulers remain gated until Committed.
- Knowledge now validates prepared destination stores and retains the vault lease
  without starting Notes/Activity/Search engines. Import completion reports
  prepared separately from active. Workspace admits its existing reviewed file
  session/recovery/LSP import methods through a finite native allowlist.
- Product renderers avoid mounting business views before commit, preventing
  browser-storage effects from bypassing native writer guards. Workspace mounts
  only reviewed migration views; Control Center exposes installation/recovery
  metadata and importer panels without Launcher or Manager execution views.
- Prepared regression cases cover native admission, future store schema and
  renderer storage isolation. Scoped Rust/test compilation passed (57.68s).
  Initial UI typecheck hit the already-fixed B07 callback type; after inheriting
  that fix, four-product TypeScript passed (17.59s). No detailed tests were run.
  Activation coordination and Windows installation acceptance remain unfinished.

### Persist native owner migration evidence (2026-09-19)

- Migration now offers an explicit per-product record action for all four owners.
  Control Center acquires authenticated native health/mapping summaries and
  verifies every listed retained backup. Matching summaries, sessions and backup
  catalogs bracket collection; changed owners, failed/missing backups, deadlines
  and changed installation journals reject the observation. Renderer arguments
  contain only the product ID, never a proof or filesystem path.
- The suite journal stores bounded metadata and the owner's retained mapping
  digest. Unknown mapping coverage remains unknown. Compare/write prevents a
  concurrent installation operation from being overwritten; repeated identical
  observations are idempotent. Recording is limited to Snapshot/Import/Validate
  and never advances activation or claims fresh legacy-source quiescence.
- Prepared a regression for phase preservation, unknown mappings, replay, stale
  journal revisions, foreign backup ownership and rejected busy/recovery states.
  Control Center Rust/test compilation and TypeScript passed in 10.24s. The
  Windows-only native coordinator is syntax formatted but awaits B08 Windows
  compilation/execution; no detailed verification ran during this implementation.

### Revalidate API originals before first import acceptance (2026-09-19)

- API apply previously accepted the reviewed destination plan without checking
  whether its original native/LevelDB sources had changed since preview. New
  native acceptance reads the verified normalized bundle, checks its retained
  backup bindings, reacquires original source handles and checks bytes/inventory.
- Windows pins source ancestors and denies competing file opens, including the
  LevelDB LOCK, throughout native intent and destination-file acceptance. Missing
  native files and selected profile inventory are rechecked without creating any
  original files. New/ambiguous browser profiles reject a previously absent source.
  Existing accepted intent resumes from its durable snapshot; later source edits
  must not strand browser acknowledgement or rollback.
- Added shared closed-source drift/inventory regressions, native JSON absence/
  drift checks and Windows write/LOCK exclusion assertions. Scoped API/shared Rust
  test compilation passed in 6.2s; platform-only unused warnings were addressed
  with Windows/test cfg boundaries. Detailed tests and Windows guard execution
  remain deferred to complete B08 verification. These owner-local guards do not
  replace fresh Suite cutover checks or claim an OS-wide transaction.

### Workspace retained mapping ledger summary (2026-09-19)

- Workspace now reports a digest/count of retained Registry reference/profile/
  template mappings, Runtime ID rows, Terminal import history and file session,
  recovery, LSP and preference receipts. Counts describe ledger entries, not
  complete transferred user-data coverage. Unselected/busy owners stay unknown;
  corrupt or inaccessible records fail instead of becoming zero.
- Runtime uses a read-only SQLite transaction including committed WAL rows, with
  schema/row/string/time limits and database identity revalidation. Metadata
  readers use native component roots and create no owner, scheduler or database.
  Workspace bounds contexts/bytes/time and returns only the digest/count, never
  source IDs, paths or content.
- Prepared WAL/uncommitted-row/schema and empty/read-only/corrupt-ledger cases.
  Initial compilation caught sha2 0.11 output formatting and a private re-export
  path; fixed those, including the same Windows-only API digest expression.
  Final Workspace/Runtime Rust/test compilation passed in 12.399s. Detailed
  regressions and native IPC remain scheduled for complete B08 verification.

### Clean first-install activation and fresh native health (2026-09-19)

- Added helper-only clean-install transitions: `--activate-clean-install` moves
  reviewed empty owners to Health; `--commit-clean-install` requires four fresh
  native health reports, closes the writer gate, preserves product data and
  durably commits before opening ordinary product writes. Package closures,
  root/namespace ownership and all four executable hashes are rechecked.
- This deliberately supports a clean first install only. Any legacy data
  namespace, retained import backup/mapping, previous generation, failed journal
  or incomplete owner review blocks this path with source-cutover-required. It
  does not stand in for the still-unfinished legacy migration/update coordinator.
- Each boundary retains a closed product-data checkpoint. Health reports carry
  native generation/session/challenge evidence and expire after five minutes.
  The UI records them sequentially to avoid concurrent journal CAS conflicts.
  Interrupted Commit can refresh expired health; a durable committed intent with
  an unwritten marker resumes coherently without pretending it rolled back.
- Control Center can reopen Health via `--suite-setup` on Updates. The public
  installer/shortcuts/update/restore flows are still unfinished; no release or
  Windows installer acceptance is claimed by these development helper commands.
- Prepared four-owner/future-time/expiry/wrong-generation/interrupted-commit
  regression. Fixed the compile-time legacy catalog include path; final scoped
  Rust/test compilation and Control Center TypeScript passed in 6.517s. Native
  helper execution, crash fixtures and detailed B08 audit remain pending.

- Clean activation also requires a complete current-user/machine ARP observation
  with no recognized prior product registrations. An installed but never-opened
  legacy app must not disappear from cleanup accounting simply because it has no
  data namespace. Unknown/inaccessible registry views block this clean-only path.
  Other users' per-user registrations remain outside this user's operation.

- Corrected native persistence of an identical owner observation: the journal
  deliberately keeps its revision unchanged, so the adapter now checks the
  expected stored digest without calling the store's revision-advancing write.
  A repeated successful observation is idempotent; concurrent changes still fail.

### Preserve Terminal originals independently of the export worker (2026-09-19)

- Workspace Terminal now retains the closed LevelDB bytes before its disposable
  worker profile can be opened/compacted. Existing worker-copy cleanup leaves this
  retained copy and the original native profile JSON untouched.
- Accepted terminal receipts bind original profile and browser metadata digests
  in the same owner document as profiles/preferences. Import restore keeps these
  bindings with the repeat receipts; semantic source fingerprints and profile IDs
  retain their existing behavior. Backup listing/verification now covers terminal
  originals. Old accepted imports without bindings report a verification failure
  rather than fabricating provenance from mutable staging files.
- Prepared accepted-backup tampering and preservation-through-restore regression.
  Workspace Rust/test compilation passed in 19.413s before the final added test/
  restore-binding assertion; those final additions were reviewed without another
  routine run. Detailed tests and Windows acquisition remain deferred to B08.

### Expose accepted Runtime SQLite backup evidence (2026-09-19)

- Workspace's backup catalog now includes the Runtime SQLite snapshot selected by
  its accepted native import digest. The receipt remains visible when retained
  bytes are missing/corrupt, so that failure cannot become an empty backup list.
- Runtime reads its own destination receipt with a read-only transaction and
  current schema/identity checks. Only bounded private stage metadata is searched;
  matching snapshot bytes are checked against the accepted digest and source
  acquisition schema. No Runtime owner, database initialization or scheduler starts.
  This verifies original SQLite bytes, not a claim about log-file coverage or
  fresh legacy state.
- Prepared accepted-vs-unaccepted, tamper/missing and future-schema regression.
  Corrected the existing crate alias found by compilation; final Workspace/Runtime
  Rust/test compilation passed in 11.619s. Detailed regressions remain deferred.
