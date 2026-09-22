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
