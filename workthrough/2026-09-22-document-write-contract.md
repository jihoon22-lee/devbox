# Document write contract (574, stage 2)

- Prerequisite: R1 merged by #575 as bb562442; CI 35670876349 and native/WSL2 35670876489 PASS.
- R2: no-clobber creation; existing save preserves the actual displaced file with Linux
  RENAME_EXCHANGE or Windows ReplaceFileW backup. Compare its native revision after
  publication; preserve evidence and the UI draft on conflict/unknown state, never blind rollback.
- R6: Unix private staging and mode preservation; Knowledge private recovery directories,
  Windows DACL-preserving replacement without ignore-ACL flags, owner/SYSTEM-only creation.
- Applied versus unknown state reaches NoteDocument; create/delete post-index failure has
  an explicit applied issue, and failed recursive deletion is unknown rather than not applied.
- Fixtures: writer injection after validation/preparation/before/after publication,
  competing creation, replacement writers, failed post-commit read, mode/DACL preservation,
  recovery access restrictions and dirty-draft/quit behavior.
- Platform contract: this is preservation plus conflict detection, not universal filesystem CAS.
  Unsupported publication fails closed. Recovery artifacts are explicitly disclosed, retained
  on conflict/interruption and removed on confirmed normal completion.
- Implementation/fixtures/documentation complete; affected verification and CI/native gates pending.

- First affected run: 248 frontend tests/builds PASS; Rust check/Clippy/fmt PASS. All
  selected native tests completed; one new nested-creation regression failed. Corrected
  nearest-existing-ancestor validation and replaced initial hard-link publication with
  no-replace rename (Linux) / MoveFileExW without replace/copy flags (Windows).
- Reverification retains the original Cargo package/feature union and runs document
  regressions plus native check/Clippy; unrelated passed frontend/common tests are retained.

- Final local result: frontend 248 PASS; original native run 2572 PASS/1 FAIL,
  corrected document suite PASS with the original Cargo feature union; final native
  check/Clippy/fmt PASS. Windows ACL/publication tests remain CI-only pending gates.

- Initial CI 35677807955 found a stale notices lock hash and a Windows DACL
  inheritance regression: private staging inherited OWNER RIGHTS leaked into the
  replacement. Linux/other Windows tests passed. Native run 35677808040 was cancelled
  because its bundled notices were obsolete.
- Fix: use same-parent replacement/backup participants on Windows and apply the source
  DACL with its protection state before publishing; then move the backup into private
  recovery. ACL regression compares all ACEs/inheritance/protection, ignoring only AI
  bookkeeping. Intermediate sibling evidence is retained and documented on failure.

- Local Windows has no Cargo, and its script policy rejected a separate synthetic
  Win32 fixture before execution. No execution-policy setting was changed. Final Windows
  behavior remains unverified locally and is covered by CI Rust tests/native acceptance.

- CI 35679657495: notices/Linux/frontend passed; Windows exposed an additional
  inheritance issue: original inherited ACEs were also retained as explicit grants.
  Native 35679657338 cancelled as superseded. Capture only explicit ACEs for an
  unprotected ACL, preserve protected ACLs, and restore inheritance after ReplaceFile
  through a pre-opened metadata handle bound to the submitted object. No path-based
  post-commit permission update can touch an external replacement.

- Final Windows Rust CI 35681022917 passed. Native 35681022912 passed installation,
  shell, cross-product and API workflows but failed actual WSL1 vault save. A local
  Win32 executable on exclusively owned synthetic WSL files confirmed GetNamedSecurityInfo
  returns ERROR_INVALID_FUNCTION and Windows private DACL creation still yields Unix 0755.
- Preserve WSL support through the existing pinned static helper: explicit Cargo protocol
  dependency, per-product hash embedding/resources, GUID-bound fixed filesystem entry,
  private Linux modes and exchange. WSL1-only ENOSYS fallback moves the displaced object
  before no-clobber link; its missing-path interval is documented, never hidden as exchange.
- Added protocol/deadline/permission/fallback races, strengthened hosted WSL native save
  acceptance to require applied outcome and retain 0600, and updated packaging/release
  resource contracts. This changes CI validators, so final correction validation is a
  single full affected/all audit plus final GitHub/Windows/WSL acceptance.

- WSL correction implementation, protocol fixtures, host deadline/pinning, native fixture,
  per-product packaging and documentation are complete. Write/create/delete adapters now
  run on blocking workers so bounded helper polling never sleeps on the async dispatcher.
  Source-only checks have no executable helper; actual WSL support is verified from the
  pinned packaged artifact. No local services, distro registrations or policies changed.

- Full local audit PASS: 1919 frontend tests, 2698 Rust tests, all builds/checks/Clippy/fmt;
  packaging/installer/release resource tests and dependency notices PASS.
- Additional musl probe of the exact document module caught a missing libc renameat2
  symbol; switched to SYS_renameat2. Windows platform modules and tests typecheck for MSVC.
  The full production helper cannot build locally without musl C tools (aws-lc); no tools
  were installed. A pure-Rust static probe using the exact document source built and passed
  an actual Windows executable -> registered current WSL GUID -> private stdin helper
  fixture: mkdir, permissions, no-clobber, exchange, sync, case preservation, 0600 file and
  0700 recovery directory. Only explicitly created temporary files were touched; no distro,
  service or policy changes. Final production static artifact and WSL1 remain hosted gates.

- The Windows interoperability fixture's operations and Linux mode checks passed; its
  immediate directory cleanup met a delayed WSL handle. The exclusively owned Windows
  temp directory was then removed and absence confirmed without changing any service.
  Native Windows staging now confirms protected DACL support before writing any content.
- After the musl syscall correction, final document regressions and workspace Clippy/fmt
  passed with the original feature union. Final Windows source checks and hosted gates remain.

- CI 35689538813 passed and the production static helper built successfully. Native
  35689538731 exposed a missing native Suite package allowlist update for Knowledge's
  helper, despite publisher/packaging fixtures passing. Admit only its exact helper pair;
  retain old Knowledge package layouts for restore/removal compatibility. Added native
  payload tests for both generations, partial pairs, wrong owners and unknown helpers.
  Await remaining independent native results before the combined correction verification.

- 같은 native 실행의 WSL 저장은 helper 없이 exe만 복사한 Knowledge migration fixture에서
  `unavailable`로 실패했다. exact-source 검증 후 helper 쌍을 함께 배치하며 Suite workflow 및
  retained-source 재개 경로도 Knowledge 리소스를 보존하도록 맞춘다. 실제 WSL 저장은 다음
  native 실행에서 다시 확인한다.
- 위 두 native 실패 수정 후 공통 자원 감독 하에 suite_package 회귀, 기존 workspace feature 통합의
  Clippy/fmt, helper staging 테스트, Suite installer/release 계약, CI scope, 변경 JS 문법 검증 PASS.
  기존 full audit 및 무관한 PASS는 유지하며 최종 hosted CI/native 수용을 다시 기다린다.
