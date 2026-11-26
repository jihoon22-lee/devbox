"""S7 daemon rows (changes §8.2): C1, CS3, CS4, CS5 (Linux check), SV1, NT1, CL1-CL3, CS10 (file part), N1-ACCEPT.

Every call goes through the owned RPC (`rpc`, authorized against the on-disk manifest), the owned
accept setup/doctor (`daemon`, wsl_daemon) or, in Windows flows only, an injected `win` object the
driver builds from its native helpers. This module names no native script and starts no Windows
program itself. CL rows use the `s7-shell` profile (bash, hooks none): no prompt reaches a tool.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time

from agents_live import SHELL_ECHO, shell_ready, text_for
from common import dump, ensure_trust, redact_text, require_lock, run_checked, trust_context, with_trust
from fixture import GIT_ENV, P_CL_FAILING_TOML, P_CL_TOML, systemd_env
from installed_binary import installed_binary

DAEMON_UNIT = 'devbox-accept@accept.service'
NOTICE_LINES = ('Copyright (c) 2015, NAVER Corporation (https://www.navercorp.com), with Reserved Font Name D2Coding.',
                'SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007')
SKIPPED = '건너뜀(변경 없음)'  # crates/cli/src/setup.rs:336
POLL = {'info': 30, 'logs': 10, 'notes': 30, 'release': 30, 'start': 30, 'prepare': 60}
LIVE = ('starting', 'running', 'restarting')
RUN_ID = re.compile('[0-9a-f]{32}')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def unit_ids(run=subprocess.run):
    """Read-only: MainPID and InvocationID of the owned daemon unit."""
    result = run(['systemctl', '--user', 'show', '-p', 'MainPID', '-p', 'InvocationID', '-p', 'ActiveState', DAEMON_UNIT],
                 capture_output=True, text=True, timeout=20, env=systemd_env())
    fields = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    failed = {'error': 'systemctl exit %d: %s' % (result.returncode, redact_text(result.stderr, 400))} if result.returncode else {}
    return {'MainPID': fields.get('MainPID'), 'InvocationID': fields.get('InvocationID'), 'ActiveState': fields.get('ActiveState'), **failed}


def run_unit_state(run_id, run=subprocess.run):
    unit = 'devbox-accept-run-' + run_id + '.service'
    result = run(['systemctl', '--user', 'show', '-p', 'ActiveState', '-p', 'LoadState', unit], capture_output=True, text=True, timeout=20, env=systemd_env())
    fields = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    return {**fields, 'error': 'systemctl exit %d: %s' % (result.returncode, redact_text(result.stderr, 400))} if result.returncode else fields


def git_owned(path, *args, run=None):
    """git in an owned fixture repo or task worktree: global/system config hidden, no hooks, no signing. A failure
    raises common.CommandFailed with the bounded, redacted stderr (fix round 1 item 6)."""
    return run_checked(['git', '-C', str(path), '-c', 'commit.gpgSign=false', '-c', 'core.hooksPath=/dev/null', *args], timeout=30, run=run,
                       text=True, env=GIT_ENV)


def error_code(error):
    return getattr(error, 'code', None)


class Background:
    """One owned call on a daemon thread (runtime.start awaits readiness)."""

    def __init__(self, function):
        self.result = self.error = None
        self.done = threading.Event()
        threading.Thread(target=self._run, args=(function,), daemon=True).start()

    def _run(self, function):
        try:
            self.result = function()
        except Exception as error:
            self.error = '%s: %s' % (type(error).__name__, str(error)[:600])
        finally:
            self.done.set()


class DaemonRows:
    def __init__(self, rpc, load, record, evidence, flow, daemon=None, win=None, pane=None, counter=None, clock=time, home=None, worktree=None):
        self.rpc, self.load, self.record_case, self.evidence, self.flow = rpc, load, record, Path(evidence), flow
        self.daemon, self.win, self.pane, self.clock = daemon, win, pane, clock
        self.counter = counter or iter(range(500, 1000)).__next__
        self.home = Path(home) if home else Path.home()
        self.worktree = worktree
        self.cases = self.evidence / 'cases'
        self.cases.mkdir(mode=0o700, exist_ok=True)

    # --- shared helpers -------------------------------------------------------------------------
    def finish(self, case, status, row, reason=None, partial=None, failure=None):
        dump(self.cases / (case + '.json'), {'case': case, 'status': status, 'reason': reason, **row})
        details = {'surface': 'installed-native' if self.win else 'wsl-artifact'}
        self.record_case(case, status, ['cases/' + case + '.json'], reason, partial=partial, failure=failure, **details)

    def windows_half(self, row_key=None):
        import evidence
        text = evidence.reason('R-IOP½')
        return text + (' (' + evidence.ROW_TEXT[row_key] + ')' if row_key else '')

    def project(self, name):
        m = self.load()
        known = (m.get('projectIdByName') or {}).get(name)
        return known or self.rpc('projects.add', {'path': m['projectPaths'][name]})['id']

    def ensure(self, row, context):
        """Fix round 1 item 5: one preview (and approval) per action, right before it, with the app's params
        (common.trust_context, common.ensure_trust). The rows are kept in the case's `trust` list."""
        result = ensure_trust(self.rpc, context)
        if row is not None:
            row.setdefault('trust', []).append(result)
        return result

    def info(self, timeout=POLL['info']):
        """Poll system.info until it answers (a restarted daemon may take a moment)."""
        deadline = self.clock.monotonic() + timeout
        while True:
            try:
                return self.rpc('system.info', {})
            except Exception as error:
                if self.clock.monotonic() >= deadline:
                    return {'error': str(error)[:300]}
                self.clock.sleep(1)

    def shell_task(self, case, project_name, row=None):
        """An s7-shell task, returned once the product has prepared its folder (see prepared())."""
        project_id = self.project(project_name)
        index = self.counter()
        worktree = True
        self.ensure(row, trust_context(project_id, 'create', worktree=worktree))
        task = self.rpc('agents.create', {'projectId': project_id, 'title': 's7-%s-%d' % (case.lower(), index), 'profile': 's7-shell',
                                          'worktree': worktree, 'prompt': text_for(self.load()['nonce'], 'P-OK'),
                                          'branch': 's7-%s-%d' % (self.load()['nonce'], index)})
        return self.prepared(task)

    def prepared(self, task, timeout=POLL['prepare']):
        """Fix round 1 item 6 (run 1's CL3 exit 128): agents.create replies `preparing` and the product adds the worktree
        in the background (crates/agents/src/create.rs:177; actions.rs:15 refuses an action while preparing). Poll
        agents.get until the task left `preparing` and its folder has the worktree's `.git` file, before any harness git
        call or action. Returns the fresh task; `failed` or the bound raises before anything touches the folder."""
        deadline = self.clock.monotonic() + timeout
        while True:
            current = self.rpc('agents.get', {'id': task['id']})
            state, tree = current.get('state'), current.get('worktreePath')
            if state == 'failed':
                raise RuntimeError('Owned task %s failed while preparing (%s)' % (task['id'], redact_text(str(current.get('cleanupError')), 300)))
            if state not in ('preparing', None) and tree and (Path(tree) / '.git').is_file():
                return current
            if self.clock.monotonic() >= deadline:
                raise TimeoutError('Owned task %s still %s after %d s; folder %s' % (task['id'], state, timeout,
                                                                                   'present' if tree and Path(tree).exists() else 'absent'))
            self.clock.sleep(0.5)

    def discard(self, agent_id, project_id, row=None):
        """As the app's review action (useReviewAction.ts:65-75): the discard preview with the task id, then
        agents.discard_check, then agents.discard with its expect."""
        self.ensure(row, trust_context(project_id, 'discard', agent_id=agent_id))
        check = self.rpc('agents.discard_check', {'id': agent_id})
        expect = {'headOid': check['headOid'], 'unmergedCommits': check['unmergedCommits']}
        if check.get('fingerprint') is not None:
            expect['discardFingerprint'] = check['fingerprint']
        try:
            return {'check': check, 'task': self.rpc('agents.discard', {'id': agent_id, 'expect': expect})}
        except Exception as error:
            return {'check': check, 'error': str(error)[:500], 'code': error_code(error)}

    def branch_listed(self, project, branch):
        return bool(git_owned(project, 'branch', '--list', branch).stdout.strip())

    def worktree_listed(self, project, folder):
        listing = git_owned(project, 'worktree', 'list', '--porcelain').stdout
        return any(line == 'worktree ' + folder for line in listing.splitlines())

    def commit_project_toml(self, name, text):
        """The only project edits a case commits: P-CL's failing/fixed teardown (fixture texts). The same text again is
        not committed (git would refuse an empty commit)."""
        path = Path(self.load()['projectPaths'][name]) / '.devbox/devbox.toml'
        if path.is_file() and path.read_text() == text:
            return False
        path.write_text(text)
        git_owned(path.parent.parent, 'add', '.devbox/devbox.toml')
        git_owned(path.parent.parent, 'commit', '-qm', 's7 cl teardown')
        return True

    def commit_file(self, tree, name, case):
        """One owned commit in a task folder: a new file whose name is the row's own, so no two tasks add the same."""
        (Path(tree) / name).write_text('s7 %s\n' % case.lower())
        git_owned(tree, 'add', name)
        git_owned(tree, 'commit', '-qm', 's7 ' + case.lower())

    def squash_merge(self, task, project_id, row):
        """As CL2 and the app's merge (useReviewAction.ts:65-75): agents.review, the merge preview with the task id,
        agents.changes, then the squash merge with its expect. With cleanup_after_close (crates/cli/src/daemon/mod.rs:140)
        the merge reply already carries the automatic cleanup's cleanupError."""
        self.rpc('agents.review', {'id': task['id']})
        self.ensure(row, trust_context(project_id, 'merge', agent_id=task['id']))
        changes = self.rpc('agents.changes', {'id': task['id']})
        expect = {'headOid': changes['headOid'], 'baseOid': changes['baseOid']}
        try:
            merged = self.rpc('agents.merge', {'id': task['id'], 'strategy': 'squash', 'message': None, 'expect': expect})
            return {'state': merged.get('state'), 'cleanupError': merged.get('cleanupError')}
        except Exception as error:
            return {'error': redact_text(str(error), 400), 'code': error_code(error)}

    def gone(self, project, task):
        """The task's folder is absent and its branch is no longer listed in the project."""
        branch = 'agent/' + (task.get('branch') or '').split('/')[-1]
        return {'worktreeGone': not Path(task['worktreePath']).exists(), 'branchGone': not self.branch_listed(project, branch)}

    # --- C1 (D1: the notice after `setup --json --restart`; the plain relaunch is recorded only) ---
    def c1(self):
        require_lock()
        m = self.load()
        config = Path(m['configPath'])
        original = m['configText'].encode()
        if config.read_bytes() != original:
            raise RuntimeError('Owned config.toml is not the fixture text; C1 does not start')
        backup = Path(m['fixtureRoot']) / 'config.toml.s7-backup'
        with backup.open('xb') as stream:
            stream.write(original)
        broken = original + b'\n[[\n'
        row = {'configPath': str(config), 'brokenSha256': sha256(broken), 'originalSha256': sha256(original), 'steps': []}
        restored = False

        def step(name, action):
            value = action()
            info = self.info()
            data = config.read_bytes()
            # Fix round 1 item 4: the product omits an empty list (crates/protocol/src/system.rs:29-31), so an absent
            # field is [] whenever system.info answered; no answer stays None.
            problems = None if 'error' in info else list(info.get('startupProblems') or [])
            row['steps'].append({'step': name, 'result': value, 'startupProblems': problems, 'fileSha256': sha256(data),
                                 'daemon': unit_ids(), 'infoError': info.get('error')})
            return problems, data
        try:
            config.write_bytes(broken)
            config.chmod(0o600)
            if self.win:
                step('app-relaunch', lambda: {'stop': self.win.stop_owned(), 'launch': self.win.launch()})
            step('setup-launch', lambda: self.daemon.setup('launch'))
            problems, data = step('setup-redeploy', lambda: self.daemon.setup('redeploy'))
            named = [p for p in problems or [] if str(config) in json.dumps(p, ensure_ascii=False)]
            parts = {'a': bool(named) and row['steps'][-1]['daemon'].get('ActiveState') == 'active',
                     'b': all(s['fileSha256'] == row['brokenSha256'] for s in row['steps'])}
            config.write_bytes(original)
            config.chmod(0o600)
            restored = True
            problems, data = step('restore-redeploy', lambda: self.daemon.setup('redeploy'))
            parts['c'] = problems == [] and sha256(data) == row['originalSha256'] and row['steps'][-1]['daemon'].get('ActiveState') == 'active'
            row['parts'] = parts
            row['launchOnlyStartupProblems'] = next((s['startupProblems'] for s in row['steps'] if s['step'] == 'setup-launch'), None)
            if self.win:  # 1.0.1: the relaunch's SC2 lines, start phases included, read after the last step (recorded only)
                row['relaunch'] = self.win.sc2(row['steps'][0]['result']['launch'], self.clock)
            failed = [k for k, v in parts.items() if not v]
            if failed:
                self.finish('C1', 'FAIL', row, 'Failed parts: ' + ','.join(failed))
            elif self.win:
                self.finish('C1', 'PASS', row, 'D1: notice after setup --json --restart; the app relaunch is recorded only')
            else:
                self.finish('C1', 'NOT_RUN', row, self.windows_half(), partial=sorted(parts))
        finally:
            if not restored:
                config.write_bytes(original)
                config.chmod(0o600)
                try:
                    row['emergencyRestore'] = self.daemon.setup('redeploy')
                except Exception as error:  # the original error stays the one raised
                    row['emergencyRestore'] = {'error': str(error)[:300]}
                dump(self.cases / 'C1-restore.json', row)

    # --- CS3 --------------------------------------------------------------------------------------
    def cs3(self):
        require_lock()
        row = {'steps': []}
        before = unit_ids()
        if self.win:
            row['relaunch'] = {'stop': self.win.stop_owned(), 'launch': self.win.launch()}
            self.info()
            row['afterRelaunch'] = unit_ids()
        report = self.daemon.setup('launch')
        after = unit_ids()
        row.update(before=before, after=after, setup=report)
        same = lambda a, b: (a['MainPID'], a['InvocationID']) == (b['MainPID'], b['InvocationID']) and a['MainPID'] not in (None, '0')
        restart = next((s for s in report.get('steps') or [] if s['id'] == 'restart'), None)
        parts = {'a': same(before, after) and (not self.win or same(before, row['afterRelaunch'])),
                 'b': bool(restart) and restart['ok'] is True and restart['message'] == SKIPPED}
        row['parts'] = parts
        if not all(parts.values()):
            self.finish('CS3', 'FAIL', row, 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))
        elif self.win:
            self.finish('CS3', 'PASS', row)
        else:
            self.finish('CS3', 'NOT_RUN', row, self.windows_half(), partial=sorted(parts))

    # --- CS4 --------------------------------------------------------------------------------------
    def cs4(self, sampled=None):
        """The startup doctor sampled at the first setup (or now), then system.doctor."""
        require_lock()
        doctor = sampled or self.daemon.startup_doctor(sampled=True)
        tools = {c['id'] for c in self.rpc('system.doctor', {}).get('checks') or []}
        descendants = doctor.get('descendants') or {}
        flagged, loops = descendants.get('flagged'), descendants.get('loops')
        row = {'startupDoctor': doctor, 'systemDoctorTools': sorted(t for t in tools if t.startswith('tool.'))}
        # M1: (a) is "no flagged descendant" (matrix CS4), proven only by at least one completed sampler pass.
        parts = {'a': flagged == [], 'b': {'tool.claude', 'tool.codex', 'tool.opencode', 'tool.pi'} <= tools}
        row['parts'] = parts
        if flagged is None:
            self.finish('CS4', 'NOT_RUN', row, 'The startup doctor was not sampled')
        elif type(loops) is not int or loops < 1:
            self.finish('CS4', 'NOT_RUN', row, 'The descendant sampler completed no pass (loops=%r)' % (loops,))
        elif all(parts.values()):
            self.finish('CS4', 'PASS', row)
        else:
            self.finish('CS4', 'FAIL', row, 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))

    # --- CS5 (Linux check; the hook half comes from U1 (b)) --------------------------------------
    def cs5(self, hook_half, upgrade_reason=None):
        require_lock()
        m = self.load()
        ids = unit_ids()
        pid = int(ids['MainPID'] or 0)
        artifact = m['linuxArtifacts']['candidate']
        link = os.readlink('/proc/%d/exe' % pid) if pid > 1 else None
        with installed_binary(Path(m['acceptBinary']), artifact['sha256'], home=self.home) as (fd, target):
            pinned = os.fstat(fd)
            running = os.stat('/proc/%d/exe' % pid) if pid > 1 else None
            same = running is not None and (pinned.st_dev, pinned.st_ino) == (running.st_dev, running.st_ino)
        expected = str(self.home / '.local/share/devbox-accept/bin' / ('devbox-' + artifact['sha256']))
        row = {'mainPid': pid, 'exe': link, 'expected': expected, 'pinnedInode': same, 'hookHalf': hook_half}
        parts = {'a': link == expected and same, 'b': hook_half}
        row['parts'] = parts
        reasons = [r for r in (upgrade_reason, None if hook_half is not None else self._reason('R-Q1')) if r]
        if parts['a'] is False or hook_half is False:
            self.finish('CS5', 'FAIL', row, 'Failed parts: ' + ','.join(k for k, v in parts.items() if v is False))
        elif reasons or not self.win:
            self.finish('CS5', 'NOT_RUN', row, ' '.join(reasons + ([] if self.win else [self.windows_half('CS5.windows')])),
                        partial=[k for k, v in parts.items() if v])
        else:
            self.finish('CS5', 'PASS', row)

    def _reason(self, code):
        import evidence
        return evidence.reason(code)

    # --- SV1 --------------------------------------------------------------------------------------
    def owned_run(self, project_id, def_id, start, timeout=POLL['start']):
        """Discover the started run through runtime.runs (recorded on disk by the RPC recorder)."""
        deadline = self.clock.monotonic() + timeout
        while True:
            items = self.rpc('runtime.runs', {'projectId': project_id, 'limit': 10}).get('items') or []
            mine = [r for r in items if r.get('defId') == def_id and r.get('projectId') == project_id]
            if mine:
                return mine[0]
            if start.done.is_set() and isinstance(start.result, dict) and start.result.get('defId') == def_id:
                return start.result
            if start.done.is_set() and start.error:
                raise RuntimeError('runtime.start failed: ' + start.error)
            if self.clock.monotonic() >= deadline:
                raise TimeoutError('Owned run %s not visible within %d s' % (def_id, timeout))
            self.clock.sleep(0.5)

    def sv1(self):
        require_lock()
        project_id = self.project('P-SV')
        row = {'trust': []}
        # Fix round 1 item 5: the app's run path (useRunActions.tsx:30-39): start; on a project trust error the run
        # preview {run, nulls}, the approval, then one retry. The refused start creates nothing (runtime/src/graph.rs:79-86).
        start = Background(lambda: with_trust(self.rpc, trust_context(project_id, 'run'),
                                              lambda: self.rpc('runtime.start', {'projectId': project_id, 'id': 's7sv1'}), row['trust']))
        run = self.owned_run(project_id, 's7sv1', start)
        run_id = run['id']
        row['runId'] = run_id
        states = []
        deadline = self.clock.monotonic() + 30
        while self.clock.monotonic() < deadline:
            detail = self.rpc('runtime.get', {'runId': run_id})['run']
            states.append(detail.get('state'))
            if detail.get('state') == 'running' or detail.get('state') not in LIVE:
                break
            self.clock.sleep(0.5)
        hits, started = [], self.clock.monotonic()
        while self.clock.monotonic() - started <= POLL['logs']:
            found = self.rpc('logs.search', {'sources': [{'kind': 'run', 'runId': run_id}], 'query': 's7-sv1-up', 'regex': False, 'limit': 10})
            hits = found.get('items') or found.get('hits') or []
            if hits:
                break
            self.clock.sleep(1)
        row['logHits'] = len(hits)
        row['logSeconds'] = round(self.clock.monotonic() - started, 3)
        row['stop'] = self.rpc('runtime.stop', {'runId': run_id})
        final = self.rpc('runtime.get', {'runId': run_id})['run']
        unit = run_unit_state(run_id)
        row.update(states=states, final=final.get('state'), unit=unit, startReturned=start.done.is_set())
        parts = {'running': 'running' in states, 'logs': bool(hits), 'stopped': final.get('state') == 'stopped' and unit.get('ActiveState') in ('inactive', None)}
        row['parts'] = parts
        self.finish('SV1', 'PASS' if all(parts.values()) else 'FAIL', row, None if all(parts.values()) else 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))

    # --- NT1 --------------------------------------------------------------------------------------
    def nt1(self, run=subprocess.run):
        require_lock()
        m = self.load()
        text = 's7-nt1-' + m['nonce']
        vault = Path(m['vaultPath'])
        artifact = m['linuxArtifacts'][m['phase']]
        env = {'HOME': str(self.home), 'PATH': '/usr/bin:/bin', 'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}'}
        with installed_binary(Path(m['acceptBinary']), artifact['sha256'], home=self.home) as (fd, target):
            result = run([target, '--instance', 'accept', 'note', 'add', text], executable='/proc/self/fd/%d' % fd, pass_fds=(fd,),
                         capture_output=True, text=True, timeout=15, env=env)
        printed = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ''
        path = Path(printed) if printed.startswith('/') else vault / printed
        inside = bool(printed) and '..' not in Path(printed).parts and path.resolve().is_relative_to(vault.resolve())
        holds = inside and path.is_file() and not path.is_symlink() and text in path.read_text(errors='replace')
        found, started = False, self.clock.monotonic()
        while self.clock.monotonic() - started <= POLL['notes']:
            items = self.rpc('search.notes', {'text': text, 'limit': 5}).get('items') or []
            if any(str(i.get('path')) in (printed, str(path)) or (str(path).endswith('/' + str(i.get('path')))) for i in items):
                found = True
                break
            self.clock.sleep(1)
        row = {'exit': result.returncode, 'printed': printed, 'insideVault': inside, 'holdsText': holds, 'searchFound': found,
               'searchSeconds': round(self.clock.monotonic() - started, 3), 'stderr': redact_text(result.stderr) if result.returncode else None}
        ok = result.returncode == 0 and inside and holds and found
        self.finish('NT1', 'PASS' if ok else 'FAIL', row, None if ok else 'note add / search.notes criteria not met')

    # --- CL1 --------------------------------------------------------------------------------------
    def wait_closed(self, agent_id, timeout=60):
        deadline = self.clock.monotonic() + timeout
        while True:
            task = self.rpc('agents.get', {'id': agent_id})
            if task.get('state') == 'closed' or self.clock.monotonic() >= deadline:
                return task
            self.clock.sleep(0.5)

    def candidates(self, project_id):
        return self.rpc('agents.cleanup_candidates', {'projectId': project_id})

    def clean(self, project_id, worktrees, branches, row):
        """agents.cleanup as the app's AgentCleanup (AgentCleanup.tsx:53-84): the cleanup preview with the selected folders
        (only when there is one), a fresh candidates read that must still list the whole selection, then the cleanup of
        exactly it. A changed list removes nothing (the app asks the user to select again)."""
        try:
            if worktrees:
                self.ensure(row, trust_context(project_id, 'cleanup', worktrees=worktrees))
            fresh = self.candidates(project_id)
            if any(w not in (fresh.get('worktrees') or []) for w in worktrees) or any(b not in (fresh.get('branches') or []) for b in branches):
                return {'cleanup': 'changed', 'candidates': fresh}
            self.rpc('agents.cleanup', {'projectId': project_id, 'worktrees': worktrees, 'branches': branches})
            return {'cleanup': 'accepted'}
        except Exception as error:
            return {'cleanup': 'refused', 'code': error_code(error), 'error': redact_text(str(error), 300)}

    def cl1(self):
        """CL1 on the product's three designed paths (plan row CL1, 10-s7-s8-release.md:1041; Ruling 84, round 2 item 7).

        (a) A task with one commit, agents.review and the squash merge: the automatic cleanup (cleanup_after_close) runs
        the passing teardown, which writes s7-teardown.txt, and the folder and branch are gone with no cleanupError.
        (d) A discard of a task whose passing teardown writes the file: accepted, not agents.state_changed, closed as
        discarded, folder and branch gone. (c) The (a) merge with P_CL_FAILING_TOML: closed, cleanupError
        agents.teardown_failed, listed in agents.cleanup_candidates. (b) With P_CL_TOML committed again, the cleanup
        preview of exactly (c)'s candidate folder and agents.cleanup of exactly it and its branch, as Settings -> Agents
        does: folder and branch gone. Record only (S7-13 Fix D; actions.rs:402-540; P-OBS-1): a discard with the failing
        teardown is refused with agents.teardown_failed and the task stays open; its state 10 s later; then, with the
        passing teardown, the same task is discarded again so nothing stays open. A row error never leaves the failing
        teardown committed."""
        require_lock()
        m = self.load()
        project = Path(m['projectPaths']['P-CL'])
        project_id = self.project('P-CL')
        row = {'trust': []}  # one preview per action, right before it (fix round 1 item 5)
        # (a): the automatic cleanup after the squash merge, passing teardown.
        a = self.shell_task('CL1', 'P-CL', row)
        self.commit_file(a['worktreePath'], 's7-cl1-a.txt', 'CL1')
        row['a'] = {'id': a['id'], 'worktree': a['worktreePath'], 'branch': a.get('branch'), 'merge': self.squash_merge(a, project_id, row)}
        closed = self.wait_closed(a['id'])
        row['a'].update(state=closed.get('state'), closedHow=closed.get('closedHow'), cleanupError=closed.get('cleanupError'), **self.gone(project, a))
        # (d): discard, passing teardown.
        d = self.shell_task('CL1', 'P-CL', row)
        row['d'] = {'id': d['id'], 'worktree': d['worktreePath'], 'branch': d.get('branch'), 'discard': self.discard(d['id'], project_id, row)}
        closed = self.wait_closed(d['id']) if 'task' in row['d']['discard'] else self.rpc('agents.get', {'id': d['id']})
        row['d'].update(state=closed.get('state'), closedHow=closed.get('closedHow'), cleanupError=closed.get('cleanupError'), **self.gone(project, d))
        try:
            self.commit_project_toml('P-CL', P_CL_FAILING_TOML)
            # (c): the same merge, failing teardown.
            c = self.shell_task('CL1', 'P-CL', row)
            self.commit_file(c['worktreePath'], 's7-cl1-c.txt', 'CL1')
            row['c'] = {'id': c['id'], 'worktree': c['worktreePath'], 'branch': c.get('branch'), 'merge': self.squash_merge(c, project_id, row)}
            closed = self.wait_closed(c['id'])
            row['c'].update(state=closed.get('state'), cleanupError=closed.get('cleanupError'), candidates=self.candidates(project_id))
            # Record only: a discard whose teardown fails.
            f = self.shell_task('CL1', 'P-CL', row)
            refused = self.discard(f['id'], project_id, row)
            self.clock.sleep(10)
            row['failingDiscard'] = {'id': f['id'], 'discard': refused, 'stateAfter10s': self.rpc('agents.get', {'id': f['id']}).get('state')}
        finally:
            self.commit_project_toml('P-CL', P_CL_TOML)
        if 'task' not in refused:  # still open: discard it again with the passing teardown
            again = self.discard(f['id'], project_id, row)
            after = self.wait_closed(f['id']) if 'task' in again else self.rpc('agents.get', {'id': f['id']})
            row['failingDiscard']['afterFix'] = {'discard': again, 'state': after.get('state'), 'closedHow': after.get('closedHow')}
        # (b): the manual cleanup of exactly (c)'s candidates.
        listed = self.candidates(project_id)
        trees = [w for w in listed.get('worktrees') or [] if w == c['worktreePath']]
        branches = [b for b in listed.get('branches') or [] if b.split('/')[-1] == (c.get('branch') or '').split('/')[-1]]
        row['b'] = self.clean(project_id, trees, branches, row)
        row['b'].update(selected={'worktrees': trees, 'branches': branches}, **self.gone(project, c))
        code = lambda value: (json.loads(value).get('code') if isinstance(value, str) and value.startswith('{') else None)
        ra, rb, rc, rd = row['a'], row['b'], row['c'], row['d']
        parts = {'a': ra['state'] == 'closed' and 'error' not in ra['merge'] and not ra['cleanupError'] and ra['worktreeGone'] and ra['branchGone'],
                 'b': rb.get('cleanup') == 'accepted' and bool(trees) and rb['worktreeGone'] and rb['branchGone'],
                 'c': (rc['state'] == 'closed' and code(rc['cleanupError']) == 'agents.teardown_failed'
                       and c['worktreePath'] in (rc['candidates'].get('worktrees') or [])),
                 'd': ('task' in rd['discard'] and rd['discard'].get('code') != 'agents.state_changed' and rd['state'] == 'closed'
                       and rd['closedHow'] == 'discarded' and rd['worktreeGone'] and rd['branchGone'])}
        row['parts'] = parts
        self.finish('CL1', 'PASS' if all(parts.values()) else 'FAIL', row, None if all(parts.values()) else 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))

    # --- CL2 --------------------------------------------------------------------------------------
    def cl2(self):
        require_lock()
        m = self.load()
        project = Path(m['projectPaths']['P-CL'])
        project_id = self.project('P-CL')
        self.commit_project_toml('P-CL', P_CL_FAILING_TOML)
        row = {'trust': []}  # one preview per action, right before it (fix round 1 item 5)
        task = self.shell_task('CL2', 'P-CL', row)  # waits for the prepared folder (fix round 1 item 6)
        tree = Path(task['worktreePath'])
        (tree / 's7-cl2.txt').write_text('s7 cl2\n')
        git_owned(tree, 'add', 's7-cl2.txt')
        git_owned(tree, 'commit', '-qm', 's7 cl')
        self.rpc('agents.review', {'id': task['id']})
        # The app's merge (useReviewAction.ts:65-75): the preview with the task id, then agents.changes, then the merge.
        self.ensure(row, trust_context(project_id, 'merge', agent_id=task['id']))
        changes = self.rpc('agents.changes', {'id': task['id']})
        expect = {'headOid': changes['headOid'], 'baseOid': changes['baseOid']}
        try:
            merged = self.rpc('agents.merge', {'id': task['id'], 'strategy': 'squash', 'message': None, 'expect': expect})
            row['merge'] = {'state': merged.get('state'), 'cleanupError': merged.get('cleanupError')}
        except Exception as error:
            row['merge'] = {'error': str(error)[:400], 'code': error_code(error)}
        closed = self.wait_closed(task['id'])
        listed = self.candidates(project_id)
        branch = task.get('branch', '')
        row['a'] = {'candidates': listed, 'state': closed.get('state'), 'cleanupError': closed.get('cleanupError')}
        off = listed.get('offBase') or []
        self.commit_project_toml('P-CL', P_CL_TOML)
        listed = self.candidates(project_id)
        trees = [w for w in listed.get('worktrees') or [] if w == str(tree)]
        branches = [b for b in listed.get('branches') or [] if b.split('/')[-1] == branch.split('/')[-1]]
        row['b'] = self.clean(project_id, trees, branches, row)
        row['b'].update(worktreeGone=not tree.exists(), branchGone=not self.branch_listed(project, 'agent/' + branch.split('/')[-1]))
        code = lambda value: (json.loads(value).get('code') if isinstance(value, str) and value.startswith('{') else None)
        parts = {'a': str(tree) in off and any(b.split('/')[-1] == branch.split('/')[-1] for b in off),
                 'b': row['b'].get('cleanup') == 'accepted' and row['b']['worktreeGone'] and row['b']['branchGone'],
                 'c': code(row['a']['cleanupError']) == 'agents.teardown_failed'}
        row['parts'] = parts
        self.finish('CL2', 'PASS' if all(parts.values()) else 'FAIL', row, None if all(parts.values()) else 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))

    # --- CL3 --------------------------------------------------------------------------------------
    def cl3(self):
        """CL3 keeps its folder for the refusal (Ruling 84, round 2 item 8). Run 2 discarded the task, and discard removes a
        submodule worktree with --force by design (the dialog warns; Fix D round 3), so nothing was left to refuse.

        The prepared folder gets `submodule update --init` and one commit; agents.discard_check is recorded while the task
        is open (c). agents.review and the squash merge follow: the automatic cleanup must be refused with
        agents.worktree_has_submodules (that cleanupError is recorded) and the folder must be listed under `submodules`.
        (a) agents.cleanup of that folder is refused the same way, its detail path is the folder, and the message holds
        the exact command. The daemon's error is {code, detail: {path}}; the message is the app's rendering of it,
        mirrored by submodule_message(). (b) Only a folder that exists and that `git -C P-SUB worktree list` lists gets the
        typed command; after it the folder must be unlisted and gone. The dialog half stays NOT_RUN (CL3.ui)."""
        require_lock()
        m = self.load()
        project = Path(m['projectPaths']['P-SUB'])
        source = Path(m['projectPaths']['sub-src'])
        git_owned(project, '-c', 'protocol.file.allow=always', 'submodule', 'add', str(source), 'sub')
        git_owned(project, 'commit', '-qm', 's7 cl3 submodule')
        project_id = self.project('P-SUB')
        row = {'trust': []}
        # Fix round 1 item 6: run 1's exit 128 was a git call on a folder the product had not made yet (`preparing`).
        task = self.shell_task('CL3', 'P-SUB', row)
        tree = task['worktreePath']
        git_owned(tree, '-c', 'protocol.file.allow=always', 'submodule', 'update', '--init')
        self.commit_file(tree, 's7-cl3.txt', 'CL3')
        row.update(task=task['id'], worktree=tree, discardCheck=self.rpc('agents.discard_check', {'id': task['id']}))
        row['merge'] = self.squash_merge(task, project_id, row)
        closed = self.wait_closed(task['id'])
        row['closed'] = {'state': closed.get('state'), 'cleanupError': closed.get('cleanupError')}
        listed = self.candidates(project_id)
        row['candidates'] = listed
        held = [w for w in listed.get('submodules') or [] if w == tree]
        # No cleanup preview here: the folder is in `submodules`, never in the app's selection (AgentCleanup.tsx:48-52),
        # and the daemon's cleanup preview refuses a folder outside `worktrees` (trust.rs:35-49). P-SUB has no
        # .devbox/devbox.toml, so there is nothing to approve.
        row['cleanupPreview'] = 'not sent: the held folder is a submodules candidate, outside the app selection'
        refusal = None
        if not held:
            row['cleanup'] = 'not sent: the folder is not listed under submodules'
        else:
            try:
                self.rpc('agents.cleanup', {'projectId': project_id, 'worktrees': held, 'branches': []})
                row['cleanup'] = 'accepted'
            except Exception as error:
                refusal = getattr(error, 'error', None) or {}
                # The domain error is {"code", "detail": {"path"}} (crates/agents/src/teardown_submodule_tests.rs:120-125).
                detail = refusal.get('detail') if isinstance(refusal, dict) else None
                row['cleanup'] = {'code': error_code(error), 'error': refusal,
                                  'appMessage': submodule_message((detail or {}).get('path') if isinstance(detail, dict) else None)}
        path = (((refusal or {}).get('detail') or {}) if isinstance(refusal, dict) else {}).get('path')
        command = "git worktree remove --force '%s'" % tree
        message = row['cleanup'].get('appMessage') if isinstance(row['cleanup'], dict) else None
        code = lambda value: (json.loads(value).get('code') if isinstance(value, str) and value.startswith('{') else None)
        parts = {'a': (code(row['closed']['cleanupError']) == 'agents.worktree_has_submodules' and bool(held)
                       and error_code_of(refusal) == 'agents.worktree_has_submodules' and path == tree and command in (message or ''))}
        # (b): the folder must be there, and listed, before anything is typed.
        before = {'folderExists': Path(tree).exists(), 'listed': self.worktree_listed(project, tree)}
        if not all(before.values()):
            row['shell'] = {'before': before, 'typed': False, 'why': 'the folder must exist and be listed before the typed command'}
        else:
            terminal = self.rpc('terminal.create', {'projectId': project_id, 'cwd': str(project), 'title': 's7-cl3'})
            tid = terminal['id']
            try:
                self.pane.begin('CL3')
                # 1.0.1 (D2): type only once the new shell has drawn its prompt and rests (bounded), so the text is not
                # echoed ahead of the prompt; then Enter only once the shell shows the command as its last line (rule (i)).
                previous, deadline = None, self.clock.monotonic() + SHELL_ECHO
                while True:
                    lines = self.pane.capture(tid)
                    if shell_ready(previous, lines) or self.clock.monotonic() >= deadline:
                        break
                    previous = lines
                    self.clock.sleep(0.2)
                self.pane.type_text(tid, command, cl3_folder=tree)
                echoed, deadline = False, self.clock.monotonic() + SHELL_ECHO
                while True:
                    self.pane.capture(tid)
                    echoed = self.pane.enter_allowed(tid, 'command')
                    if echoed or self.clock.monotonic() >= deadline:
                        break
                    self.clock.sleep(0.2)
                if echoed:
                    self.pane.key(tid, 'Enter', 'command')
                    deadline = self.clock.monotonic() + 30
                    while (self.worktree_listed(project, tree) or Path(tree).exists()) and self.clock.monotonic() < deadline:
                        self.clock.sleep(0.5)
                row['shell'] = {'before': before, 'typed': True, 'echoed': echoed, 'listedAfter': self.worktree_listed(project, tree),
                                'folderExists': Path(tree).exists()}
                if not echoed:
                    row['shell']['why'] = 'the shell did not show the typed command as its last line; no Enter was sent'
            finally:
                self.rpc('terminal.close', {'id': tid})
        parts['b'] = all(before.values()) and row['shell'].get('listedAfter') is False and row['shell'].get('folderExists') is False
        parts['c'] = row['discardCheck'].get('hasSubmodules') is not None
        row['parts'] = parts
        if not all(parts.values()):
            self.finish('CL3', 'FAIL', row, 'Failed parts: ' + ','.join(k for k, v in parts.items() if not v))
        else:
            self.finish('CL3', 'NOT_RUN', row, self._ui('CL3.ui'), partial=sorted(parts))

    def _ui(self, key):
        import evidence
        return evidence.ui(evidence.ROW_TEXT[key])

    # --- CS10 (Windows flow only) -----------------------------------------------------------------
    def cs10(self):
        require_lock()
        settings = self.win.settings_drvfs()
        stage = self.win.stage_drvfs()
        original = settings.read_bytes()
        backup = stage / 'settings.json.s7-backup'
        with backup.open('xb') as stream:
            stream.write(original)
        row = {'settings': str(settings), 'stop': self.win.stop_owned()}
        restored = False
        try:
            settings.write_bytes(b'{')
            before = sorted(p.name for p in settings.parent.iterdir())
            row['launch'] = self.win.launch()
            self.clock.sleep(30)
            row['stopAfter'] = self.win.stop_owned()
            after = sorted(p.name for p in settings.parent.iterdir())
            row.update(brokenSha256=sha256(b'{'), afterSha256=sha256(settings.read_bytes()), namesBefore=before, namesAfter=after)
            ok = row['afterSha256'] == row['brokenSha256'] and before == after
            settings.write_bytes(original)
            restored = True
            row['restoredSha256'] = sha256(settings.read_bytes())
            if not ok:
                self.finish('CS10', 'FAIL', row, 'The broken settings.json changed or a file appeared beside it')
            else:
                self.finish('CS10', 'NOT_RUN', row, self._ui('CS10.ui'), partial=['file'])
        finally:
            if not restored:
                settings.write_bytes(original)

    # --- N1-ACCEPT (Windows flow only) ------------------------------------------------------------
    def n1_accept(self, candidate_sha, show):
        require_lock()
        root = self.win.install_drvfs()
        row = {}
        ok = True
        for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):
            path = root / name
            regular = path.is_file() and not path.is_symlink()
            data = path.read_bytes() if regular else b''
            expected = sha256(show(candidate_sha + ':' + name))
            row[name] = {'regular': regular, 'sha256': sha256(data) if regular else None, 'expected': expected}
            ok = ok and regular and sha256(data) == expected
            if name == 'THIRD_PARTY_NOTICES.md':
                flat = ' '.join(data.decode('utf-8', 'replace').split())
                row[name]['lines'] = {line: line in flat for line in NOTICE_LINES}
                ok = ok and all(row[name]['lines'].values())
        self.finish('N1-ACCEPT', 'PASS' if ok else 'FAIL', row, None if ok else 'Installed notices differ from the candidate')


def error_code_of(error):
    return (error or {}).get('code') if isinstance(error, dict) else None


# The app's rendering of agents.worktree_has_submodules (app/src/rpc/agentMessages.ts:99-101, with shellQuote from
# app/src/rpc/messageDetail.ts:14), as Settings -> Agents shows it: the title, then the action naming the folder.
SUBMODULE_TITLE = '하위 모듈이 있는 작업 폴더는 devbox가 지우지 않습니다.'
SUBMODULE_ACTION = '하위 모듈의 변경은 커밋하고 커밋은 push한 뒤 프로젝트 폴더의 터미널에서 git worktree remove --force {path}로 직접 지우세요.'


def submodule_message(path):
    quoted = "'" + path.replace("'", "'\\''") + "'" if isinstance(path, str) and path else '<폴더>'
    return SUBMODULE_TITLE + ' ' + SUBMODULE_ACTION.format(path=quoted)
