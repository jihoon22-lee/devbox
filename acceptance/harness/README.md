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
- **Not wired into CI.** `test_s7_harness.py` runs by hand (`python3 -B -m unittest test_s7_harness`), as §1 says.
- The text below still says rc.2 in places and uses that run's paths; it is kept as written.

### Changes for 1.0.1 (the code here differs from the imported run copy)

Each item is a lesson of the 1.0.0 acceptance. The helper hashes changed, so the first 1.0.1 run needs a full code
review again (§2.3).

- **Artifact folders and branches are run arguments.** `--candidate-artifacts <name>` (and `--baseline-artifacts`) name
  the folder under the evidence root; `--candidate-branch` and `--baseline-branch` name the dispatch branch (defaults
  `main` and `v1/s7-rc1-accept`). All four are in the approval's `binding`, so the reviewer still pins them. No helper
  names a candidate any more.
- **Keys settle before the next input.** codex 0.172.0 lost the first character of text typed 4–8 ms after Escape. See
  "Keys and typed commands" under the rules.
- **One launcher for the owned executable.** See "Starting the owned app" under the rules.
- **PowerShell helpers are pure ASCII.** `validate.py` fails on a non-ASCII byte in a `.ps1` without a UTF-8 BOM.
- **The app's own update, and foreign files.** The new `update` flow and the cases UP1, UP2 and A7 (§7).

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
  - Since 1.0.1 the Windows fingerprint also holds the running prod app (PID and start time of every `Devbox.exe` in a
    prod install folder) and the `devbox://` open command as text. A prod process that is gone, restarted or new is the
    difference kind `prodProcessChanged`; a changed command is `prodProtocolChanged`. Each fails the comparison (M4, A6
    and every check during a flow), with both sides kept in `prodComparison`. The harness never starts or stops a prod
    app, so a prod app the user starts or quits during a run also shows there: the controller reads the two sides.
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
- **Keys and typed commands (1.0.1).**
  - After any key or paste, the next text, key or paste to the same pane first waits until 0.6 s (`KEY_SETTLE`) have
    passed, then re-captures until two captures in a row are equal, at most 3 s after the key (`KEY_SETTLE_MAX`).
    `Pane` does this itself, so no row can skip it. `Live.press` settles right after a key that is not Enter, so a
    product send (`agents.send`, MCP) that follows a key waits too. The case keeps `keySettles`: the count, the longest
    wait, and the waits that ended without two equal captures.
  - The Enter of a typed command (rule (i)) needs a capture taken after the text, whose **input row is exactly the
    text**. The input row is the last captured line that starts with the tool's prompt marker (`❯` for claude, `›` for
    codex). `quit/quit`, `quit`, or the text anywhere else on the screen do not pass. `Pane.enter_allowed` checks it,
    so every caller is covered; `Live.command` checks the same before it asks.
  - opencode and pi have no known marker (no capture of their input row exists yet). For them one more whole line must
    equal the text than in the capture taken right before typing, and no line may hold the text inside other text.
    The first live run that reaches H3, H3X-ENV or H4 must read `noEcho` and `inputRows` in the case file.
  - A command whose row never was the text gets no Enter. On a known input row the harness clears what its own typing
    left with C-u (`cleared`), so the next command does not land on it. The case keeps `noEcho` and `inputRows`.
  - CL3's shell command is typed only once the new terminal shows a prompt on two captures in a row (at most 10 s). It
    gets its Enter only once the last line ends with the command; above it the command may stand at most once more, as a
    whole line (the terminal's echo of text that arrived before the prompt). Without that the row keeps `echoed: false`
    and (b) fails. CL3 now captures its shell terminal, so the stop patterns apply to that screen too.
- **Starting the owned app (1.0.1).**
  - The owned executable starts only through `process-launch.ps1`: `CreateProcess` with a handle list of exactly NUL and
    the two log files (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), so the app inherits nothing else. That covers the launch
    and, since 1.0.1, the installed self-check (`Invoke-OwnedLoggedProcess`: a bounded wait and the exit code on the
    creation handle).
  - Never start a program with `Start-Process -RedirectStandardOutput/-RedirectStandardError` (or `-NoNewWindow`) from a
    PowerShell that WSL started. That call creates the child with every inheritable handle of the PowerShell process,
    the WSL interop pipes included, and the calling WSL command does not return before the child exits. `validate.py`
    fails on such a line in a helper.
  - The installers and the uninstaller start with a plain `Start-Process -PassThru` (ShellExecute: no handle is
    inherited) and are waited for with a bound.
  - `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (the debug port) and `DEVBOX_UPDATE_FIXTURE` are set only in the launching
    helper's own environment block and restored right after the start; never a user or machine variable.
- **PowerShell helper encoding (1.0.1).** Windows PowerShell 5.1 reads a `.ps1` without a BOM in the ANSI code page
  (CP949 on the acceptance PC), so a non-ASCII character silently becomes other characters. Every helper `.ps1` is pure
  ASCII; `validate.py` (`scan_ps1_encoding`) allows only pure ASCII or a UTF-8 BOM with valid UTF-8.
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
| `native-lifecycle.ps1` | Install from the owned stage, upgrade, launch, stop, uninstall with the 30 s registration poll; `update-begin`, `update-observe`, `update-adopt` and `-Foreign` for the update flow. |
| `process-launch.ps1` | Handle-list launcher (start, and start-and-wait for the self-check). |
| `native-cdp.ps1` | The one client of the owned app's WebView2 debug port: five update commands, bounded waits. |
| `update_fixture.py` | The accept update fixture as the product reads it, and the update shape. Pure. |
| `update_flow.py` | The `update` flow: the app's commands in the screen's order, the runner watch, the connection gap, UP1 and UP2. |
| `foreign_files.py` | The owned marker files of UP2 and A7: plant, read back, compare, remove. Pure. |
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
CANDART=windows-run-rc2                   # the candidate's folder name under $S7 (--candidate-artifacts)
RC1ART=windows-run-rc1                    # the baseline's folder name under $S7 (--baseline-artifacts)
CANDBRANCH=v1/s7-rc2                      # the branch of the candidate's dispatch (--candidate-branch; default main)
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
   gh run download $CANDRUN -R jihoon22-lee/devbox -D $S7/$CANDART/artifacts
   ```

   Each `artifacts/` directory must hold exactly `devbox-setup/`, `devbox-setup-accept/` and `devbox-linux/`.
   The folder name is one plain path component (letters, digits, `.`, `_`, `-`; not `runs` or `harness`), a real
   directory directly under `$S7`, never a symlink. The driver refuses anything else before the lock.
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
python3 -B - "$H" "$S7" "$CAND" "$RC1" "$CANDART" "$RC1ART" <<'EOF'
import sys; sys.path.insert(0, sys.argv[1])
import evidence
s7, cand, rc1, candart, rc1art = sys.argv[2:]
for name, sha, run, version in ((candart, cand, 41423905881, '1.0.0-rc.2'), (rc1art, rc1, 41374466543, '1.0.0-rc.1')):
    d = evidence.artifact_dir(s7, name)
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
             "windowsLocal": "/mnt/c/Users/<user>/AppData/Local", "distro": null,
             "candidate": {"sha": "...", "runId": 41423905881, "branch": "v1/s7-rc2", "artifacts": "windows-run-rc2",
                           "artifactSha256": "..."},
             "baseline": null}}
```

- **`helperSha256`** equals `runtimeHelperSha256` in `helper-manifest.json`. It covers every non-test `.py` and
  `.ps1` plus `matrix-text.json`. It changes only when the code changes.
- **`binding`** is what the driver computes:
  - `sourceBindingSha256` is `sha256sum $H/source-binding.json`;
  - `answersSha256` is `sha256sum $H/answers.json`;
  - `cases` is the flow's list from `evidence.FLOWS`, or the `--cases` subset in run order;
  - `branch` is that side's `--candidate-branch`/`--baseline-branch` (the run's `head_branch` must equal it);
  - `artifacts` is that side's folder name under the evidence root (`--candidate-artifacts`/`--baseline-artifacts`);
  - `artifactSha256` is `evidence.artifact_digest` of that side's `artifacts/` directory;
  - `baseline` is `null` for the fresh flows, or `{sha: RC1, runId: 41374466543, branch, artifacts, artifactSha256}`
    for the two-sided flows (`wsl-upgrade`, `upgrade`, `update`);
  - `updateShape` exists only for the `update` flow (`same-version` or `next-version`, §7);
  - `distro` is `--distro` for the Windows flows (the WSL distro the owned app is pointed at) and `null` for the WSL
    flows.

The next command prints the values the driver will compute for one flow. Pass the same arguments as the run.

```sh
python3 -B - "$H" --flow wsl-fresh --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
  --candidate-branch $CANDBRANCH --windows-local "$WINLOCAL" <<'EOF'
import json, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import driver, evidence
args = driver.parse(sys.argv[2:])
root = driver.evidence_root(args.evidence_root, create=False)
sides = driver.sides_of(args)
for side, row in sides.items():
    row['artifactSha256'] = evidence.artifact_digest(evidence.artifact_dir(root, row['artifacts']))
print(json.dumps({'helperSha256': evidence.helper_hashes(driver.BASE),
                  'binding': evidence.approval_binding(driver.BINDING_FILE.read_bytes(), args.flow, args.cases,
                                                       Path(args.answers).read_bytes(), sides['candidate'],
                                                       sides.get('baseline'), args.windows_local,
                                                       update_shape=args.update_shape, distro=args.distro)}, indent=2))
EOF
```

This printout is not an approval. The reviewer:
1. reviews the code;
2. checks each value against its own source: the two `sha256sum` values, the CI run, the receipts from step 1.4,
   the answers;
3. only then writes the file with `"status": "APPROVE"`.

After the first review, a new flow, SHA, run, branch, artifact folder, case list or answer changes only `binding`,
which is a small diff to re-review. For the `update` flow this printout leaves the version pair unchecked; the driver's
gates check it (`update_fixture.check_shape`). The driver refuses before the lock when either part differs. The message starts with
`helperSha256:` or `binding:`.

### 2.4 Run

Each run is one command under one lock, from the worktree, with no `__pycache__` in `$H`. Every flow needs
`--candidate-artifacts` and `--windows-local`. The two-sided flows also need `--baseline-artifacts`. The Windows flows
also need `--distro`. `--candidate-branch` defaults to `main`. `--baseline-branch` defaults to `v1/s7-rc1-accept` for the
upgrade flows and has no default for `update`, which refuses to start without it.

```sh
cd $W
# wsl-fresh
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow wsl-fresh --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
    --candidate-branch $CANDBRANCH --windows-local "$WINLOCAL"
# wsl-upgrade
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow wsl-upgrade --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
    --candidate-branch $CANDBRANCH --baseline-sha "$RC1" --baseline-run $RC1RUN --baseline-artifacts $RC1ART \
    --windows-local "$WINLOCAL"
# fresh (only with Q0 true): Wave 1
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow fresh --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
    --candidate-branch $CANDBRANCH --windows-local "$WINLOCAL" --distro "$WSL_DISTRO_NAME"
# upgrade (only with Q0, Q1 and Q2 true): Wave 2
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow upgrade --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
    --candidate-branch $CANDBRANCH --baseline-sha "$RC1" --baseline-run $RC1RUN --baseline-artifacts $RC1ART \
    --windows-local "$WINLOCAL" --distro "$WSL_DISTRO_NAME"
# update (only with Q0 true; 1.0.1, §7): the baseline accept install updates itself to the candidate installer.
# $BASE, $BASERUN and $BASEART are the baseline build's SHA, run and folder; both builds are on main here.
env $ENVV PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify-resources.py --lock windows-acceptance -- \
  python3 -B $H/driver.py --flow update --candidate-sha "$CAND" --candidate-run $CANDRUN --candidate-artifacts $CANDART \
    --baseline-sha "$BASE" --baseline-run $BASERUN --baseline-artifacts $BASEART --baseline-branch main \
    --update-shape same-version --windows-local "$WINLOCAL" --distro "$WSL_DISTRO_NAME"
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
- In `fresh`, every installed case needs A1 and A2, and A6, A7 and M4 need A1.
- In `update`, UP2 needs UP1.

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

     - If it has `sealedPaths` but no `resealed` key, and its `flow` is an upgrade flow (`wsl-upgrade` or `upgrade`,
       or `update` once its update began: the run folder has `native-update-begin.json`), reseal first, then clean up, each under the lock (review 5, R5-M1). The run stopped between the rc.2 install and
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

**After a killed `update` run (1.0.1).** If the run folder has `native-update-begin.json`, the product's update may
have replaced the owned executable and relaunched the app with the debug port open, so the plain stop alone refuses
(the executable pin). With the controller's decision, run these four in this order, each under the lock, and stop at the
first one that fails (everything is then retained and reported):

```sh
PSX=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
HW=$(wslpath -w $H); OWN=$(wslpath -w $S7/runs/<run>/ownership-owned-<token>.json)
L="[Console]::OutputEncoding=New-Object Text.UTF8Encoding; & ([ScriptBlock]::Create([IO.File]::ReadAllText('$HW\native-lifecycle.ps1'))) -Ownership '$OWN' -HelperRoot '$HW'"
$PSX -NoProfile -NonInteractive -Command "$L -Action 'update-adopt' -WaitSeconds '600'"   # waits for the runner, records the relaunched app
$PSX -NoProfile -NonInteractive -Command "$L -Action 'stop-owned'"                        # exact PID, path and start time only
$PSX -NoProfile -NonInteractive -Command "$L -Action 'assert-stopped'"                    # no owned app, runner or debug port left
$PSX -NoProfile -NonInteractive -Command "$L -Action 'uninstall'"
```

If `stop-owned` reports an app that started after it, repeat `update-adopt` and `stop-owned` before `assert-stopped`
(the driver's own teardown does this up to three times). Then the WSL steps of item 1 follow.

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
{"candidate": {"sha": "<CAND>", "runId": 41423905881, "version": "1.0.0-rc.2", "artifacts": "windows-run-rc2"},
 "baseline": {"sha": "<RC1>", "runId": 41374466543, "version": "1.0.0-rc.1", "artifacts": "windows-run-rc1"}}
```

```sh
(cd $H && S7_LIVE_ARTIFACTS=1 python3 -B -m unittest test_s7_harness.Provenance.test_live_artifacts -v)
```

## 7. The update flow and the foreign files (1.0.1)

Two release gates of 1.0.1 have never run on an installed app: the app's own update, and what the installer and
the uninstaller do to files they did not install.

### 7.1 What the product allows (read from the source; re-check the lines against the bound SHA)

- **The fixture.** An accept build (`cfg devbox_update_fixture`, `app/src-tauri/build.rs:19-30`) reads the environment
  variable `DEVBOX_UPDATE_FIXTURE` once at start (`app/src-tauri/src/platform/update.rs:26-48`) and opens that folder
  with `Fixture::open(path, source)` (`app/src-tauri/src/core/update/fixture.rs:35-95`). `source` is the 40-hex
  `SOURCE_SHA` the build was compiled with. The folder holds:
  - `owner.json` (at most 2048 bytes, exactly these keys): `schema` 1, `channel` `"accept"`, `sourceSha` equal to the
    **running** build's `SOURCE_SHA`, a stable `currentVersion`, `metadataSha256`, `installerSha256`;
  - `metadata.json` (at most 1 MiB): a GitHub release. `release::select` (`release.rs:46-100`) needs a tag `v<stable>`
    above `currentVersion`, not a draft or prerelease, and exactly one asset: id above 0, name
    `devbox-<version>-setup.exe`, state `uploaded`, the exact API and download URLs, a size of 1 byte to 256 MiB, and a
    digest `sha256:<64 hex>`;
  - `installer.exe`: exactly that size and sha256.
- **The source SHA.** `Fixture::open` compares only `owner.json`'s `sourceSha` with the running build
  (`fixture.rs:48`). Nothing names the installer's source SHA: the installer is bound by size and sha256 only. So a
  version-N build can be given the installer of another source SHA.
- **The version.** After the installer ran, the runner (a copy of the old executable) starts the app again only when
  the installed `devbox-version.txt` equals `expected_version` (`platform/update_runner/execute.rs:315-324`,
  `core/self_check.rs:4-20`). For an accept fixture install that is the **runner's own** `CARGO_PKG_VERSION`, not the
  release version (`execute.rs:221-227`). An installer of another version ends `relaunch` / `failed`, and no app
  starts. So the accept channel cannot run N to N+1; the closest shape is **the same version from two source SHAs**
  (`--update-shape same-version`). The driver refuses a pair with different bound versions before the lock.
  `--update-shape next-version` exists for a product build that compares against the release version; today it fails.
- **The commands.** The Settings screen calls, in this order (`app/src/features/settings/nativeUpdate.ts:155-162,
  202-226`, `Update.tsx:96-113`, `UpdateConfirmation.tsx:97, 124-128`): `update_status` (and the `devbox://update`
  event), `update_check`, `update_download {generation}`, `update_install_summary {generation}`,
  `update_install_confirm {generation, confirmationId}`.
- **No user gesture is needed.** No update command checks the calling window or a gesture
  (`platform/update.rs:93-142`, `platform/update_install.rs:15-47`). The quit approval is not a click either:
  `update_install_confirm` starts a quit round, the app sends `devbox://prepare-quit` to every document window
  (`platform/quit.rs:138-175`), and each window's own code answers with `quit_prepared`. The harness calls the five
  commands in the main window and never `quit_prepared`, `quit_window_ready` or `quit_app`: every window approves
  through its real code. The dialog's own rule that the daemon counts must have loaded
  (`UpdateConfirmation.tsx:117-123`) is screen-only; the harness records that `agents.list` answered instead.

### 7.2 What the `update` flow does

1. The gates, the prod fingerprints and the stage, as in `upgrade`. Both sides are bound; the baseline is the running
   build.
2. `native-lifecycle install` of the baseline, then `update_fixture.build_fixture` writes `<owned stage>\update\`
   (`installer.exe` is a byte copy of the candidate's accept installer, checked against its receipt).
3. `native-lifecycle launch -CdpPort <port> -UpdateFixture <owned stage>\update`. The port is 39000–39899, derived from
   the run token; the launch refuses an occupied port.
4. One owned shell task (`s7-shell`: bash, no tool) is created; its tmux session identity is read. With UP2 selected the
   `update` set of foreign files is planted in the install folder.
5. `update-begin`, then the five commands through `native-cdp.ps1`, then `update-observe` about once a second until the
   product's runner has ended and the relaunched app runs; `update-adopt` records that app and pins the installed bytes.
6. The instance proof and the reseal for the candidate, `update_status` on the new app (its `currentVersion`),
   `installed-identity` (the bundled Linux binary equals the candidate's artifact) and the prod comparison.

Guards before the update can change anything: `update-begin` refuses unless NSIS's memory of the product (the default
value of `HKCU\Software\github\Devbox Accept`, as every earlier run recorded it) is this run's owned install path
and `%LOCALAPPDATA%\Devbox Accept` does not exist; the launch refuses unless the fixture's `installer.exe` is byte for
byte the staged candidate installer. If `%LOCALAPPDATA%\Devbox Accept` exists afterwards, `update-adopt` and
`assert-stopped` fail and everything is retained.

The watch ends as a success only on the runner's own `installed` record, or when the relaunched app has removed the
stage folder it read (the product does that only for an installed record); a `failed`, `unknown` or `cancelled`
record is read first.

**UP1** passes only when all of these hold: the installed executable changed and the runner recorded the install
(`installerRan`); exactly one relaunched app answers in its `main` window (`newAppStarted`); the reported version,
`devbox-version.txt` and the bundled version equal the candidate's bound version (`version`); the relaunched build is
the candidate (`identity`); the WSL boot id and PID 1 are unchanged (`distroUp`); the tmux session, pane and task are
the same (`tmuxSurvived`); the connection gap was measured (`connectionGapMeasured`); prod is unchanged.

`identity` and `installerRan` decide which build runs. The product shows its source SHA nowhere at run time, so
`identity` is three facts together (`identityHow` in `cases/UP1.json`): the installed `devbox-linux` is byte for byte
the candidate run's Linux artifact, whose receipt names the candidate SHA; the daemon the relaunched app deployed and
runs is that same artifact (the instance proof of the candidate phase); and the relaunched app refuses the fixture it
inherited, whose `owner.json` names the baseline SHA, so its compiled `SOURCE_SHA` is not the baseline's. In the shape
`same-version` both builds report the same version, so `version` is recorded and must hold but is left out of
`decidingParts`. The gap is a measurement without a threshold: inside WSL, every 100 ms,
from the first sample without the old app's `devbox bridge` process to the first sample with a new one.

**UP2** reads the `update` set back after the installer ran and removes exactly that set. **A7** (also in `fresh`)
plants the `uninstall` set before the plain uninstall; `native-lifecycle uninstall -Foreign` reads it back before the
owned tree removal, and removes exactly that set. Both pass only when every file is still there with the same length
and sha256.

A7's read counts only after the uninstaller's own end. The NSIS uninstaller goes on in a copy of itself after
`uninstall.exe` returned, so the helper first waits (read-only, at most 30 s) until `uninstall.exe` is gone and the
uninstaller removed its `Uninstall\Devbox Accept` registration, then reads the files twice, 1 s apart. A7 fails when
that end was not seen (`uninstallerEnded`) or the two reads differ (`stable`), whatever the files looked like.

Limits to state with a result:
- `distroUp` is weak evidence for the product's keep-alive: the harness itself runs inside the distro. The observed
  `wsl.exe ... --exec sleep 900` child of the runner (`keepAlive`) is the direct evidence. It ends by itself within
  15 minutes and is recorded, not stopped.
- The debug-port app is ended in teardown: `update-adopt` (it waits up to 600 s for the product's runner and installer,
  then records the relaunched app), `stop-owned`, then `update-observe` again; the round repeats, at most three times,
  while a runner, an installer or an owned app process is present. `assert-stopped` then requires that no process runs
  the owned executable, no update runner or installer runs, and the debug port is closed. If any of that fails,
  nothing is uninstalled, the stage is kept whole, the resources are retained and the run is FAIL.

### 7.3 The first live run must check

- `validate.py --powershell-parse` passes for `native-cdp.ps1`, `native-lifecycle.ps1` and `process-launch.ps1`. The
  parse does not compile the C# in `process-launch.ps1`: the first `install` does (the self-check runs through it).
- `native-install-*.json` shows `installedSelfCheck: PASS` and a self-check record with `startedUtc`.
- `cases/UP1.json`: `commands` reached `update_install_confirm`; `runner.runnerSeen` and `installerSeen`; whether the
  runner could start at all (it needs `CREATE_BREAKAWAY_FROM_JOB`; a WSL-started PowerShell may put the app in a job).
- The debug port listener descends from the owned app (the helper refuses otherwise), and the relaunched `--minimized`
  app still has a main window page.
- `connection.gapSeconds` is not null (the bridge process must match `devbox bridge` with `DEVBOX_INSTANCE=accept`).
- Whether the product's installer kept the owner marker in the install folder (`adoption.markerRecreated`).
- That the relaunched app really started. A new build reads the old build's `updates\install-*\owner.json`, `spawn.json`
  and `outcome.json` before anything else (`core/update/runner_recovery.rs:61-130`, `platform/update_runner.rs:309-338`).
  If it cannot read them (a record field the two source SHAs do not share, `runner_files.rs:46-57`), it shows a message
  box and exits with 3. The harness then sees a process without a page: `newAppStarted` fails, and teardown records and
  stops that process, so no dialog is left on the desktop.
- A7's `comparison.uninstallerEnded` and `waitMs`: whether the 30 s wait for the uninstaller's end is enough on this PC.

### 7.4 Choosing the baseline

The baseline must be an accept build with the candidate's version from another source SHA. What the run then proves
is the **baseline's** runner (prepare, quit round, commit, installer, relaunch) and the **candidate's** start after it.
To cover the released 1.0.0 as far as this channel allows, take as the baseline the earliest commit that already has
the candidate's version and still has 1.0.0's update code (`app/src-tauri/src/core/update/`,
`app/src-tauri/src/platform/update*`): then the runner that executes is 1.0.0's code. The N to N+1 version check itself
(`execute.rs:221-227`) cannot be exercised on the accept channel; it stays a prod-only path.
