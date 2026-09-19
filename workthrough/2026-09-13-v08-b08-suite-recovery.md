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
