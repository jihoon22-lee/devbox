# S7-14 accept acceptance harness (1.0.0-rc.2)

## About this copy (2026-11-25, ADR 0018)

This folder is the versioned source of the Windows acceptance harness. Until 2026-11-25 it lived only in the Git
common-dir of one PC.

- **Imported as run.** The code here is byte-for-byte the harness that accepted the 1.0.0 code candidate `cc704e6f`
  (runs `cc704e6f-a17b22a5` and `cc704e6f-282b323d`); the staged helper hashes equal that run's approved `helperSha256`.
- **Run copy.** Runs still happen from the run copy in the Git common-dir (`devbox-verification/s7/harness/`), because
  the driver keeps its run state next to the code: `answers.json` (the user's answers), `source-binding.json`,
  `review-approval.json` and `helper-manifest.json`. Those files are not in the repository.
- **Before a run:** copy the code from here to the run copy, or confirm with `diff -r` that it is the same. Any
  difference is reviewed by the independent reviewer together with the binding (§2.3).
- **Known divergence.** `evidence.py` names the candidate's artifact folder as a literal (`ARTIFACT_DIRS`). Each new
  candidate therefore needs a one-line edit in the run copy, reviewed with that run's binding. Deriving the folder from
  the run arguments is an open follow-up, as are the tool-screen rows that send keys right after Escape.
- **Not wired into CI.** `test_s7_harness.py` runs by hand (`python3 -B -m unittest test_s7_harness`), as §1 says.
- The text below still says rc.2 in places and uses that run's paths; it is kept as written.

This harness accepts the 1.0.0-rc.2 candidate on this PC. It is built from the reviewed S6-6b harness and follows
`.superpowers/sdd/10-s7-s8-release/s7-14-harness-changes.md` and `s7-14-matrix.md`. Only the controller runs it, and
only after an independent review of these files.

## Rules the harness keeps

- **One lock.** Every run takes one `windows-acceptance` lock from `scripts/verify-resources.py`. The lock fd is
  handed to sub-helpers, and each live entry point checks it first.
- **Owned resources only.** Every accept path must be absent before the run and is removed after it. Anything
  uncertain is retained and reported as FAIL.
  - The owned fixture projects live in `/tmp/devbox-s7-fixture-<token>/`, outside any repository: mode 0700, with the
    owner marker, named in the manifest (`fixtureRoot`) and proven absent before the run (`fixtureRootAbsentBefore`).
    The owned cleanup removes it only after the manifest's location, name, mode and marker checks pass again. After a
    WSL restart has emptied `/tmp`, the owned recovery goes on without it (§5, N1).
  - The parents the product creates when they are absent (for example `~/.local/share/devbox` or a
    `*.target.wants` folder) are recorded as `absentParentsBefore`. Cleanup removes only those, and only when empty.
  - The evidence stays under `$S7/runs/`.
- **Prod is read-only.** WSL prod (and Windows prod in the Windows flows) is fingerprinted before and after.
- **Parameters, not literals.** The candidate and the baseline are arguments, never literals in helper code.
- **Two-part approval.** `helperSha256` covers the code. `binding` covers the run inputs: source binding, flow,
  cases, answers, SHAs, runs, artifact bytes and LocalAppData.
- **Answers gate actions.** `answers.json` holds the user's answers:
  - Q0 gates the Windows flows;
  - Q1 and Q2 gate the trust keys;
  - Q3 gates K1X-TMUXUP;
  - Q4 gates W1-D;
  - Q5 gates the approval Enter.
- **Interop attribution.** `interopBroken` is a bound input. While it is true, a stall that broken interop explains
  records NOT_RUN with R-IOP-STALL and keeps the original failure as counts only. Any other failure stays FAIL.
- **Real tools, tightly held.** Tools run only in owned accept tasks, with the fixed texts and the turn budgets.
  - tmux is used only as `/usr/bin/tmux -L devbox-accept`, on recorded targets, with the Enter guard.
  - Tool trust is read as one bool (the `hasTrustDialogAccepted` key of `~/.claude.json`) and one sha256
    (`~/.codex/config.toml`). Neither file is printed, copied or written.
- **Stop patterns.** A capture that matches `update`, `login`, `terms`, `onboarding` or `api-key` (for pi also
  `pi-trust`) stops the block: no further key is sent and the remaining rows are NOT_RUN (R-PROMPT). The patterns
  themselves are unchanged in fix round 2; the controller rules on `api-key` after run 3.
  - **A stop-pattern change (Ruling 83).** pi's passive update banner is an allowed notice: it is not a stop,
    for pi only. It is exactly these three lines in a row, each whole once surrounding spaces and box padding are
    stripped, where `<x.y.z>` is `\d+\.\d+\.\d+`:

    ```text
    Update Available
    New version <x.y.z> is available. Run pi update
    Changelog: https://pi.dev/changelog
    ```

  - A pi capture whose `update` matches all fall inside such banners does not stop, and the other patterns are
    still checked on it. The banner goes to the case as `passiveNotices`: its redacted lines and `seen`, the number
    of captures that showed it.
  - **A second allowed notice (Ruling 154).** claude's login-expiry warning is not a stop, for claude only. It is
    exactly this line, where `<N>` is `\d+` and the word is `day` or `days`, the whole line once surrounding spaces
    and box padding are stripped:

    ```text
    ⚠ Your login expires in <N> days · run /login to renew
    ```

  - A claude capture whose `login` matches all fall on such lines does not stop; any other `login` match in the same
    capture still stops. It goes to `passiveNotices` like pi's banner: its redacted line and `seen`.
  - **The cut banner (Ruling 87).** A capture is at most the last 60 lines, so the banner can lose its first line
    above the top edge. That edge is the 60-line window's, or the pane's own top: run 3's captures are 50 rows, with
    no history above. When the capture's rows 0 and 1 are exactly the banner's last two lines, stripped the same way,
    those two rows are the banner too, whatever the capture's length. Its notice has `"cut": true`. Only the top edge
    cuts: the bottom is the live screen.
  - Every other update text still stops, for every tool. So does the banner, whole or cut, on any other tool, and on
    pi when another line of the same capture also matches `update`. The banner's last two lines anywhere but rows 0
    and 1, and a lone `Update Available`, stop too.
- **Folder-trust rows (H1X-TRUST, H2X-TRUST).**
  - (b)'s attention bound is `TIMEOUTS['trustAttention']`, 7.0 s, measured from the first capture that showed the
    trust screen (Ruling 86). It is the product's output detection, "2 to about 6 s" (`crates/agents/src/watch.rs:56-57`;
    a third watcher tick, `:111-116`, `:177`, `:191`), plus 1 s for the screen to settle, the poll and the RPC.
  - codex's output-detected waits whose parts count `attention` use the same 7.0 s, from the `waiting_checks` call
    right after the screen showed: H2X-RESUME (b), H2X-SCREENS and P1X (a). H2X-FEEDBACK and the shell rows leave
    `attention` out of their parts and still record it against 2 s. The hook-detected wait, claude's permission
    prompt in P1 (and U1 추가), keeps 2 s.
  - Each `waiting_checks` call adds its `checks` dict, every key and value, to the case's `checks` list. The case also
    keeps the last call's `attentionSeconds` and `attentionBoundSeconds`.
  - A tool that ends before its trust screen ends the row NOT_RUN with `<row>.toolExit`. "Ends" is the product's
    end line on the pane, or a task that is `failed`, `review` or `closed`, or has `toolExited`. The row keeps
    `toolExit`: the exit code and the redacted last 15 lines, trailing blank lines dropped.
  - The codex `config.toml` sha256 is read before `agents.create`. Every path, absent and tool exit included, keeps
    the claude flag (`trustEntryPresent`, `trustFlagSet`) or the codex hash (`configSha256`: before, after,
    unchanged).
- **Which binary each task ran (`toolBinary`, P-DEF-1).** At the first `agents.get` that names a real-tool task's
  terminal, the harness reads that task's owned `~/.local/share/devbox/accept/agents/<id>/start.sh` once
  (`crates/agents/src/prepare.rs:220`). It must be a regular file of at most 64 KiB; it is opened without following
  a link and never executed. The creating case keeps only:
  - its `export PATH=` lines, redacted;
  - the program the tool line runs: the first word, or the absolute path for a shared folder;
  - for a bare name, the realpath of `<PATH-front folder>/<program>`.

  A read that fails is kept as `error`.
- **CS4.** The descendant sampler flags any process whose argv holds the word `devbox-probe` (the product probe's
  `$0`: `bash -lic … devbox-probe <program>`, `crates/agents/src/command_path.rs:181`), a `bash -ic`, or an agent
  CLI `--version`.
- **CL1 and CL3 on the product's designed close paths (Ruling 84).**
  - CL1 (a): a task with one commit, `agents.review` and the squash merge; the automatic cleanup runs the passing
    teardown. (c): the same with the failing teardown leaves `cleanupError` `agents.teardown_failed` and a cleanup
    candidate. (b): with the passing teardown committed again, the cleanup preview and `agents.cleanup` of exactly
    (c)'s candidates. (d): a discard with the passing teardown.
  - CL1 record only (`failingDiscard`): a discard with the failing teardown is refused with
    `agents.teardown_failed`; the task's state 10 s later; then the second discard after the fix. A row error never
    leaves the failing teardown committed.
  - CL3 never discards. The squash merge's automatic cleanup must be refused with `agents.worktree_has_submodules`,
    and the folder must stay under `submodules`. `agents.cleanup` of it must be refused the same way, naming the
    folder. The command is typed only while the folder exists and `git worktree list` lists it.
  - The daemon's refusal carries only the code and the path. The message checked in CL3 (a) is the app's rendering
    of it (`app/src/rpc/agentMessages.ts:99-101`), mirrored by the harness.
- **No bytecode.** The driver refuses to start while `$H/__pycache__` exists, and runs every child Python with `-B`.
- **The remaining send window (review M11).** Right before each `agents.send` or MCP send, the harness reads
  `agents.get` and captures the pane again; it sends only while the state is `waiting` and no stop pattern shows,
  and records each check as `sendGate`. A screen that appears after that re-check and before the send is still not
  seen. The re-check narrows the window; it does not close it.
- **Things it never does:**
  - download, dispatch or select a workflow run, or write through `gh`;
  - install or run the prod installer;
  - change or bypass the PowerShell execution policy;
  - send input to a window;
  - run `wsl --shutdown`;
  - touch Docker, the firewall or any network setting.

## Files

| File | Role |
|---|---|
| `driver.py` | The one entry point: gates, the four flows, teardown and the case matrix. |
| `common.py` | Lock proof, evidence root, `interop_probe`, the product probe bytes, exact identity stop. |
| `evidence.py` | Case lists, `NEEDS`, gates, `record` with the answer and interop rules, receipt and artifact checks, the two-part approval. |
| `matrix-text.json` | The matrix sentences for NOT_RUN reasons and record-only rows (bound by `helperSha256`). |
| `answers.json` | The user's Q0–Q5 answers and the controller's `interopBroken` state (bound by `binding`). |
| `fixture.py` | Accept path allowlist, `bind_source`/`verify_source`, the owned fixture plan (`wsl` and `windows` modes), stage. |
| `bind_source.py` | Writes `source-binding.json` for the exact candidate (and baseline) SHA. It uses `git show` only. |
| `wsl-owned.py` | Prod WSL fingerprint, accept instance proof, the owned-only RPC allowlist, seal and reseal, exact cleanup, the K1X tmux restart. |
| `wsl_daemon.py` | WSL flows: the product's own install and exec scripts from the bound `deploy.rs`, `setup --json`, startup doctor. |
| `agents_live.py` | Real-tool rows: fixed texts, `TurnBudget`, `Pane` (tmux and the Enter guard), stop patterns, `trust_probe`. |
| `daemon_rows.py` | Daemon rows: CS3/CS4/CS5/CS10, SV1, NT1, CL1–CL3, C1, N1-ACCEPT. |
| `mcp_stdio.py` | The owned `devbox --instance accept mcp` client. It allows only `devbox_agents` and `devbox_agent_send`. |
| `w1_windows.py` | W1 Windows exec rows: control files, the W1-F console watcher, the W1-H console sampler, the exact holder stop. |
| `a4_windows_service.py` | A4 Windows service readiness. |
| `g1_live_member.py`, `g2_probe_timing.py` | G1 and G2 interop gates. The driver runs them in the `fresh` flow. |
| `native-observe.ps1` | Read-only: preflight, prod fingerprint, identity, residue, `console-windows`, `console-watch`. |
| `native-lifecycle.ps1` | Install from the owned stage, upgrade, launch, stop, uninstall with the 30 s registration poll. |
| `process-launch.ps1` | Handle-list launcher. |
| `teardown.py` | Failure-tolerant finalize, prod comparison, stage cleanup. |
| `coordinator.py`, `installed_binary.py` | Phase gates; the pinned Linux binary. |
| `validate.py` | Static checks. `--write-manifest` writes `helper-manifest.json`. |
| `test_s7_harness.py` | Pure unit tests. They use fakes and temporary folders only. |

## 0. Variables

Use one shell for everything below.

```sh
export PYTHONDONTWRITEBYTECODE=1          # no __pycache__ may appear in $H; the driver refuses while one exists
W=/home/jihoon/projects/devbox-wt/v1-s7-rc2
C=/home/jihoon/projects/devbox/.git
S7=$C/devbox-verification/s7
H=$S7/harness
ENVV="XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus DEVBOX_VERIFY_CGROUP=required"
CANDRUN=41423905881                       # windows.yml run of v1/s7-rc2
RC1RUN=41374466543                        # windows.yml run of v1/s7-rc1-accept (success)
RC1=$(git -C $W rev-parse 'v1.0.0-rc.1^{commit}')   # must print 28eb2a9111c83f4345e1b7ed2ae04d6849fa116e
WINLOCAL=/mnt/c/Users/<windows user>/AppData/Local # see `ls /mnt/c/Users`; no Windows program is needed
```

## 1. Preparation (controller, no lock)

1. **Wait for the candidate CI run.** Run 41423905881 must be completed with conclusion `success`. Only then
   take the candidate SHA from the branch:

   ```sh
   gh api repos/jihoon22-lee/devbox/actions/runs/$CANDRUN --jq '[.status, .conclusion, .event, .head_branch, .head_sha]'
   # expect ["completed","success","workflow_dispatch","v1/s7-rc2","<same as below>"]
   CAND=$(git -C $W rev-parse v1/s7-rc2)
   ```

   If the run failed, stop. The driver's `verify_run` refuses any run that did not succeed.
2. **Download the candidate artifacts.** The harness never downloads. The rc.1 artifacts are already in
   `$S7/windows-run-rc1/artifacts/`.

   ```sh
   gh run download $CANDRUN -R jihoon22-lee/devbox -D $S7/windows-run-rc2/artifacts
   ```

   Each `artifacts/` directory must hold exactly `devbox-setup/`, `devbox-setup-accept/` and `devbox-linux/`.
3. **Make the evidence root private.** `evidence_root` refuses an `s7/` that is not mode 0700. Today it is 0755.

   ```sh
   chmod 700 $S7
   ```

4. **Check both artifact directories.** This is pure. The command is below the list, unindented, because
   it is a heredoc.
5. **Run the unit tests and the static checks.** Run these after any harness change.

   ```sh
   (cd $H && python3 -B -m unittest test_s7_harness -v)
   python3 -B $H/validate.py --worktree $W --source-sha "$CAND" --write-manifest
   ```

   - `validate.py` exits 0 with `"status": "PASS"` and writes `helper-manifest.json`.
   - Without `--powershell-parse`, the report records `"PowerShellParse": "NOT_RUN: needs interop; ..."`. The WSL
     flows never run a `.ps1`.
   - Before a Windows flow, rerun it under the lock with the parse, after interop is restored:

     ```sh
     cd $W && env $ENVV python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
       python3 -B $H/validate.py --worktree $W --source-sha "$CAND" --powershell-parse --write-manifest
     ```

Step 4, the artifact check:

```sh
python3 -B - "$H" "$S7" "$CAND" "$RC1" <<'EOF'
import sys; sys.path.insert(0, sys.argv[1])
import evidence
s7, cand, rc1 = sys.argv[2:]
for d, sha, run, version in ((s7 + '/windows-run-rc2/artifacts', cand, 41423905881, '1.0.0-rc.2'),
                             (s7 + '/windows-run-rc1/artifacts', rc1, 41374466543, '1.0.0-rc.1')):
    chain = evidence.verify_artifact_dir(d, sha, run, version)
    print(version, chain['installerName'], chain['installerSha256'], evidence.artifact_digest(d))
EOF
```

## 2. Each flow: bind, approve, run

Run the flows in this order:
1. `wsl-fresh`;
2. `wsl-upgrade`;
3. after the user restores interop (Q0 yes) and the interop probe passes, `fresh`;
4. then `upgrade`.

With Q0 false, a Windows flow records every case NOT_RUN (R-IOP) and changes nothing.

### 2.1 Bind the source (one binding per flow)

The fresh flows bind the candidate only (`baseline: null`). The upgrade flows also bind rc.1.

`bind_source.py` refuses to overwrite `source-binding.json`. Before rebinding, move the previous file into the
run that used it (`runs/<sha8>-<hex8>/`). It is that run's evidence.

```sh
# wsl-fresh and fresh
python3 -B $H/bind_source.py --worktree $W --candidate-sha "$CAND"
# wsl-upgrade and upgrade
mv $H/source-binding.json $S7/runs/<previous run>/source-binding.json
python3 -B $H/bind_source.py --worktree $W --candidate-sha "$CAND" --baseline-sha "$RC1"
```

A new binding changes `binding.sourceBindingSha256`, so that flow's binding part must be approved again.

### 2.2 Answers

`answers.json` holds the user's answers. The controller records them:
- Q0–Q5 as booleans;
- `interopBroken`;
- `"source": "user"`, `"recordedBy": "controller"` and `recordedAt`.

Today Q0–Q4 are false, Q5 is true (Ruling 79) and `interopBroken` is true. The controller changes a value only on a user answer, or, for
`interopBroken`, only after the user restores interop and a Windows-flow interop probe passes. Any change breaks
`binding` (`answersSha256`) until it is approved again.

### 2.3 Approval (the independent reviewer)

`$H/review-approval.json` must be exactly:

```json
{"status": "APPROVE",
 "helperSha256": {"<file>": "<sha256>", "...": "..."},
 "binding": {"sourceBindingSha256": "...", "flow": "...", "cases": ["..."], "answersSha256": "...",
             "windowsLocal": "/mnt/c/Users/<user>/AppData/Local",
             "candidate": {"sha": "...", "runId": 41423905881, "artifactSha256": "..."},
             "baseline": null}}
```

- **`helperSha256`** equals `runtimeHelperSha256` in `helper-manifest.json`. It covers every non-test `.py` and
  `.ps1` plus `matrix-text.json`. It changes only when the code changes.
- **`binding`** is what the driver computes:
  - `sourceBindingSha256` is `sha256sum $H/source-binding.json`;
  - `answersSha256` is `sha256sum $H/answers.json`;
  - `cases` is the flow's list from `evidence.FLOWS`, or the `--cases` subset in run order;
  - `artifactSha256` is `evidence.artifact_digest` of that side's `artifacts/` directory;
  - `baseline` is `null` for the fresh flows, or `{sha: RC1, runId: 41374466543, artifactSha256}` for the upgrade
    flows.

The next command prints the values the driver will compute for one flow. Pass the same arguments as the run.

```sh
python3 -B - "$H" --flow wsl-fresh --candidate-sha "$CAND" --candidate-run $CANDRUN --windows-local "$WINLOCAL" <<'EOF'
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import driver, evidence
args = driver.parse(sys.argv[2:])
root = driver.evidence_root(args.evidence_root, create=False)
sides = driver.sides_of(args)
for side, row in sides.items():
    row['artifactSha256'] = evidence.artifact_digest(root / evidence.ARTIFACT_DIRS[side] / 'artifacts')
print(json.dumps({'helperSha256': evidence.helper_hashes(driver.BASE),
                  'binding': evidence.approval_binding(driver.BINDING_FILE.read_bytes(), args.flow, args.cases,
                                                       Path(args.answers).read_bytes(), sides['candidate'],
                                                       sides.get('baseline'), args.windows_local)}, indent=2))
EOF
```

This printout is not an approval. The reviewer:
1. reviews the code;
2. checks each value against its own source: the two `sha256sum` values, the CI run, the receipts from step 1.4,
   the answers;
3. only then writes the file with `"status": "APPROVE"`.

After the first review, a new flow, SHA, run, case list or answer changes only `binding`, which is a small diff
to re-review. The driver refuses before the lock when either part differs. The message starts with
`helperSha256:` or `binding:`.

### 2.4 Run

Each run is one command under one lock, from the worktree, with no `__pycache__` in `$H`. Every flow needs
`--windows-local`. The Windows flows also need `--distro`.

```sh
cd $W
# wsl-fresh
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow wsl-fresh --candidate-sha "$CAND" --candidate-run $CANDRUN --windows-local "$WINLOCAL"
# wsl-upgrade
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow wsl-upgrade --candidate-sha "$CAND" --candidate-run $CANDRUN \
    --baseline-sha "$RC1" --baseline-run $RC1RUN --windows-local "$WINLOCAL"
# fresh (only with Q0 true): Wave 1
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow fresh --candidate-sha "$CAND" --candidate-run $CANDRUN \
    --windows-local "$WINLOCAL" --distro "$WSL_DISTRO_NAME"
# upgrade (only with Q0, Q1 and Q2 true): Wave 2
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow upgrade --candidate-sha "$CAND" --candidate-run $CANDRUN \
    --baseline-sha "$RC1" --baseline-run $RC1RUN --windows-local "$WINLOCAL" --distro "$WSL_DISTRO_NAME"
```

**What the driver checks:**
- **Before the lock:** the arguments, the approval, the artifact digests, the answers and the source binding.
- **Under the lock:**
  - HEAD is `$CAND` on `v1/s7-rc2` with a clean status;
  - `verify_source` and the gates again;
  - the CI run through the read-only `gh api`;
  - the receipts.

**`--cases`** picks a subset in the flow's order. The subset is bound, so it needs its own approved binding.
- `fresh --cases G1,G2` exercises only the interop gates.
- In `fresh`, every installed case needs A1 and A2, and A6 and M4 need A1.

**Rows already passed.** Rows that passed with `surface: "wsl-artifact"` are not repeated on `installed-native`
(Ruling 79 D3).

**C1 (Ruling 79 D1)** passes on these, after `setup --json --restart`:
- the notice;
- the config file bytes are unchanged;
- the state is normal after the restore.

A plain app relaunch is recorded only.

## 3. W1 (라): a controller step, not a harness case

W1 (라) needs interop (R-IOP). Run it with Wave 1, after Q0, and before the `fresh` flow. It needs no install.

1. Under the default lock:

   ```sh
   cd $W && source ~/.cargo/env
   python3 -B scripts/verify-resources.py -- cargo test -p devbox-cli --lib -j 2 --no-run
   ```

   Note the test binary path from the `Executable unittests src/lib.rs (...)` line.
2. Inside the `windows-acceptance` session:

   ```sh
   env $ENVV python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
     env DEVBOX_TEST_WINDOWS_POWERSHELL=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe \
         'DEVBOX_TEST_WINDOWS_DIR=C:\Users\<windows user>\AppData\Local\Temp' \
     <test binary> windows_exec::probe_tests::the_holder_receipt_waits_for_an_orphan_that_outlives_the_bootstrap --ignored --exact
   ```

   If this session already holds the lock, run the same `env ... <test binary> ...` without the wrapper. A nested
   wrapper waits, then exits 75.

PASS only when that run's output shows `1 passed; 0 failed` for that test.

## 4. Evidence

Each run writes to `$S7/runs/<sha8>-<hex8>/` (mode 0700):

| File | Content |
|---|---|
| `acceptance.json` | The matrix. Every selected case has a status; unselected cases are NOT_RUN. |
| `driver-result.json` | Phases, cleanup, `retainedResources`, `prodComparison`, `acceptAbsentAfter`. |
| `review-approval-used.json` | The approval this run used. |
| `cases/<ID>.json`, `cases/<ID>-mcp.json`, `cases/<ID>-step.json` | Per-case evidence, the MCP pairs (redacted), and a failing step. |
| `cases/<ID>-error.json` | The error that ended a case as FAIL: type, bounded message, exit code and redacted stderr. |
| `cases/<ID>-stop.json` | A stop pattern that ended a block: the pattern's name, the lines that matched it (`matches`: at most 5, each with 2 lines above and below; `matchCount`) and the last 15 captured lines. All of it is redacted and cut to 400 characters a line. The case row keeps the same as `stop`. |
| `console-windows-samples.json` | W1-F (a): the watcher's achieved interval, skipped samples, the first sample's windows and every console window that appeared later. |
| `fixture-cleanup.json`, `cleanup-product-steps.json` | The owned cleanup: units, the fixture root, the parents removed or kept. |
| `ownership-owned-*.json` | The owned-resource manifest. |
| `product-scripts.json`, `mcp-closed.json` | The bound product scripts used, and the closed MCP processes. |

The driver prints `{status, evidenceDirectory, cases}` at the end.

**Status rules:**
- A FAIL in any case makes the run FAIL.
- NOT_RUN rows carry the matrix's sentence. A partial row shows `NOT_RUN(부분: ...)`.
- A row that sends to a tool records `sendGate`: for each send, the state and the stop check just before it, and
  whether it was sent.
- Case rows also keep, where they apply: `checks`, `attentionBoundSeconds`, `toolExit`, `trustFlagSet`,
  `configSha256`, `toolBinary` and `passiveNotices` (see §"Rules the harness keeps").
- W1-F (a) comes from one long-lived watcher (`native-observe.ps1 -Action console-watch`). It samples every
  250 ms from before the first W1 run to the end of W1-F. Any console window that appears after the first sample
  fails (a), whoever owns it, the user's own windows included. (a) is NOT_RUN when the largest gap is over 500 ms, a
  sample is skipped or cut, or the watcher ends early.

## 5. If a run is interrupted

- **Teardown runs in `finally`.** It handles every failure the driver can catch: the seal, the owned cleanup, the
  prod comparison and the accept-absent proof.
- **If the driver process itself was killed**, the next run's preflight refuses while accept residue exists. That
  is fail-closed. Act only when teardown did not finish, and never delete anything by hand. Then (review C6):
  1. Read that run's `ownership-owned-<token>.json`.
     - If the run folder has no `seal-owned-resources.json` and the run reached install or setup, seal first, then
       clean up, both under the lock (review 6, R6-M1). The driver writes that file only after its seal returns
       (`driver.py:380-386`). `sealedPaths` alone does not show the seal, because the stage writes its own folders
       there first. The CLI `seal` does not write the file, so seal at most once: if cleanup then refuses, stop and
       leave the residue for the controller.

       ```sh
       cd $W && env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
         python3 -B $H/wsl-owned.py seal --ownership $S7/runs/<run>/ownership-owned-<token>.json
       cd $W && env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
         python3 -B $H/wsl-owned.py cleanup --ownership $S7/runs/<run>/ownership-owned-<token>.json
       ```

     - If it has `sealedPaths` but no `resealed` key, and its `flow` is an upgrade flow (`wsl-upgrade` or `upgrade`),
       reseal first, then clean up, each under the lock (review 5, R5-M1). The run stopped between the rc.2 install and
       its reseal, and cleanup alone refuses at a unit file or accept path that the rc.2 phase changed or created.
       Reseal is not a second seal: it accepts a changed file only as a setup-managed unit that starts with the setup
       marker.

       ```sh
       cd $W && env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
         python3 -B $H/wsl-owned.py reseal --ownership $S7/runs/<run>/ownership-owned-<token>.json
       cd $W && env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
         python3 -B $H/wsl-owned.py cleanup --ownership $S7/runs/<run>/ownership-owned-<token>.json
       ```

     - Otherwise, when `sealedPaths` is present, run only the `cleanup` command. Never seal again: a second seal would
       re-hash the files.
     - Any refusal leaves the residue for the controller to review.
     - **After a WSL restart (N1).** A restart empties the tmpfs `/tmp`, and the fixture root with it. The same
       commands still work: `seal`, `reseal` and `cleanup` go on when `lstat` proves the root absent.
       - They skip only the root's own steps: its projects with their tasks, runs and terminals, its worktrees, the
         vault and the marker. The units' stop still ends those processes.
       - Every other owned resource is removed by its manifest and seal rows, as before. The printed result says
         `"fixtureRoot": "absent"`, and so does `fixture-cleanup.json`.
       - A root that exists but is not the owned 0700 directory with the matching marker still refuses. Every other
         action, the driver's own teardown included, still needs the root.
  2. For a Windows flow, report the Windows residue read-only. The paths are `nativePreflight.installPath` and
     `nativePreflight.windowsRoot` from that run's ownership manifest. The command is below this list.
- **There is no standalone native cleanup entry point.** Removing Windows residue needs the controller's
  decision.

The read-only residue report (step 2):

```sh
PSX=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
HW=$(wslpath -w $H)
$PSX -NoProfile -NonInteractive -Command "[Console]::OutputEncoding=New-Object Text.UTF8Encoding; & ([ScriptBlock]::Create([IO.File]::ReadAllText('$HW\native-observe.ps1'))) -Action residue -InstallPath '<installPath>' -WindowsRoot '<windowsRoot>'"
```

## 6. Optional live-artifact unit test

The suite skips the live-artifact test unless both of these are present:
- `S7_LIVE_ARTIFACTS=1`;
- `$S7/test-binding.json`.

```json
{"candidate": {"sha": "<CAND>", "runId": 41423905881, "version": "1.0.0-rc.2"},
 "baseline": {"sha": "<RC1>", "runId": 41374466543, "version": "1.0.0-rc.1"}}
```

```sh
(cd $H && S7_LIVE_ARTIFACTS=1 python3 -B -m unittest test_s7_harness.Provenance.test_live_artifacts -v)
```
