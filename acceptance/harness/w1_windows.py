"""S7 W1 rows (changes §8.3): Windows runs of the owned P-WIN project on the installed accept app.

W1 (가)(나), (사), (바), (마), (아); (다) only with Q4. Observation is read-only: the owned
copy's mapped `control` directory through drvfs (windows_observation.rs:50-87), the injected
`win.console_windows` sampler for W1-H's probe processes (native-observe.ps1 -Action console-windows),
for W1-F (a) one long-lived `win.console_watch` (-Action console-watch, fix round 1 I3), and, for W1-AB
(Ruling 147), the owned accept instance DB (accept_db), opened `mode=ro` only (read_phase: one SELECT, never a
write or a PRAGMA). The only process this module stops is one run's holder, by its exact identity from
`<nonce>.assigned.json` (`win.stop_exact`).
"""
import contextlib
import json
from pathlib import Path, PurePosixPath
import re
import sqlite3
import threading
import time
import urllib.parse

from common import dump, ensure_trust, redact, redact_text, require_lock, trust_context
from fixture import W1_EDITED_SERVICE, p_win_toml

CONTROL_FILE = re.compile(r'([0-9A-Za-z_-]+)\.(assigned\.json|grant\.json|closed\.json|emptied\.json|holder-ready)')
WINDOWS_PATH = re.compile(r'([A-Za-z]):\\([^\x00]*)')
TIMEOUTS = {'start': 210, 'assigned': 35, 'assignedBound': 60, 'probeWatch': 240, 'release': 70, 'releaseBound': 120, 'closedAfterEnd': 30}
ORPHAN_SECONDS = 20  # s7w1a sleeps 20 s (fixture W1_TASKS)
LIVE = ('starting', 'running', 'restarting')
# Ruling 147: the attempt phases (windows_copy_store.rs:70-86) that keep the copy reader while the orphan runs; `closed`
# (store::observed_closed, after the receipt) and `revoked` (a command denied) are not among them.
READER_KEPT = ('intent', 'assigned', 'committed')
# The owned accept instance's data dir (system.info dataDir, wsl-owned instance_proof); its DB is devbox.db (core paths.rs:92).
ACCEPT_DATA = '.local/share/devbox/accept'
# I3: the W1-F watcher samples every 250 ms; the matrix bound (W1 (바), "every 500 ms") is the largest gap allowed.
# 3600 s is the watcher's own upper bound, above the W1-AB..W1-F worst case (about 2100 s of start and wait bounds).
WATCH = {'intervalMs': 250, 'boundMs': 500, 'seconds': 3600, 'firstSample': 60}


def exit_code(run):
    """Run.exit is an ExitRecord {attemptId, code, signal?, endedMs, by} (crates/runtime/src/exit_record.rs:32)."""
    record = run.get('exit') if isinstance(run, dict) else None
    return record.get('code') if isinstance(record, dict) else None


def log_epoch(ts):
    """LogLine.ts as epoch seconds, or None when it is not ISO-8601."""
    from datetime import datetime
    try:
        return datetime.fromisoformat(str(ts).replace('Z', '+00:00')).timestamp()
    except ValueError:
        return None


def drvfs(windows_path):
    """`C:\\a\\b` -> `/mnt/c/a/b` (default automount root); no `..`, no other form."""
    match = WINDOWS_PATH.fullmatch(str(windows_path))
    if not match:
        raise ValueError('Windows drive path required')
    parts = [p for p in match.group(2).split('\\') if p]
    if '..' in parts or '.' in parts:
        raise ValueError('Windows path must not contain . or ..')
    return Path(str(PurePosixPath('/mnt', match.group(1).lower(), *parts)))


def run_token(windows_root):
    """The run token of the owned Windows copy root `C:\\dev\\devbox-s7-accept-<token>`, the console-windows nonce.

    It matches processes whose command line names the copy root in plain text. The holder starts with
    -EncodedCommand, so holders are matched by their assigned.json identity instead (ControlWatch)."""
    match = re.fullmatch(r'C:\\dev\\devbox-s7-accept-([0-9a-z-]{6,64})', str(windows_root or ''))
    if not match:
        raise ValueError('Owned Windows copy root required')
    return match.group(1)


def control_dir(run, windows_root):
    """`<base>\\files` is runtime.get windows.directory; the private control dir is `<base>\\control`."""
    directory = (run.get('windows') or {}).get('directory')
    if not directory or not directory.startswith(windows_root + '\\') or not directory.endswith('\\files'):
        return None
    return drvfs(directory[:-len('\\files')]) / 'control'


class ControlWatch:
    """Every 100 ms, names and mtimes of the run's control files (read-only drvfs listing)."""

    def __init__(self, directory, clock=time, interval=0.1):
        self.directory, self.clock, self.interval = Path(directory), clock, interval
        self.seen = {}
        self.assigned = {}
        self.stop = threading.Event()
        threading.Thread(target=self._loop, daemon=True).start()

    def _loop(self):
        while not self.stop.is_set():
            self.sample()
            self.stop.wait(self.interval)

    def sample(self):
        try:
            entries = list(self.directory.iterdir()) if self.directory.is_dir() and not self.directory.is_symlink() else []
        except OSError:
            entries = []
        now = self.clock.monotonic()
        for entry in entries:
            match = CONTROL_FILE.fullmatch(entry.name)
            if not match or entry.is_symlink():
                continue
            key = (match.group(1), match.group(2))
            if key not in self.seen:
                try:
                    self.seen[key] = {'firstSeen': now, 'mtime': entry.stat().st_mtime}
                except OSError:
                    continue
            if match.group(2) == 'assigned.json' and match.group(1) not in self.assigned:
                try:
                    value = json.loads(entry.read_text(encoding='utf-8-sig'))
                    self.assigned[match.group(1)] = {'pid': int(value['pid']), 'creationTime': str(int(value['creationTime'])),
                                                     'runId': value.get('runId')}
                except (OSError, ValueError, KeyError):
                    pass

    def first(self, kind, nonce=None):
        """The earliest firstSeen of kind; with nonce, of that nonce's file only (Ruling 147: every run of the owned copy
        shares this control dir). The listing is copied first: the watcher thread adds to it."""
        times = [row['firstSeen'] for (n, name), row in list(self.seen.items()) if name == kind and (nonce is None or n == nonce)]
        return min(times) if times else None

    def nonce_for(self, run_id):
        """The nonce whose parsed assigned.json names run_id (the first seen), else None."""
        if run_id is None:
            return None
        return next((nonce for nonce, row in list(self.assigned.items()) if row.get('runId') == run_id), None)

    def result(self, since):
        self.stop.set()
        rel = lambda t: None if t is None else round(t - since, 3)
        return {'files': [{'nonce': n, 'kind': k, 'afterSendSeconds': rel(v['firstSeen']), 'mtime': v['mtime']} for (n, k), v in sorted(self.seen.items())],
                'holders': self.assigned}


def first_of(watch, kind, nonce):
    """W1-AB: watch.first for this run's nonce only; None without a nonce (never another run's file)."""
    return None if nonce is None else watch.first(kind, nonce)


def accept_db(m, home=None):
    """Ruling 147: the owned accept instance DB, `<home>/.local/share/devbox/accept/devbox.db`, that read_phase reads. Its
    folder must be one the fixture proved absent before the run (m['absentBefore'], so this run's), a real directory, and
    the DB a regular file; neither may be a symlink."""
    folder = Path(Path.home() if home is None else home) / ACCEPT_DATA
    db = folder / 'devbox.db'
    listed = m.get('absentBefore')
    if not isinstance(listed, list) or str(folder) not in listed:
        raise ValueError('The accept data folder was not proved absent before the run: ' + str(folder))
    if folder.is_symlink() or not folder.is_dir():
        raise ValueError('The accept data folder is not a real directory: ' + str(folder))
    if db.is_symlink() or not db.is_file():
        raise ValueError('The accept DB is not a regular file: ' + str(db))
    return db


def open_ro(db):
    """The only way this module opens the accept DB: a `mode=ro` URI, so SQLite refuses every write (Ruling 147)."""
    return sqlite3.connect('file:' + urllib.parse.quote(str(db)) + '?mode=ro', uri=True, timeout=1)


def read_phase(db, nonce):
    """The phase of nonce's attempt row (runtime_windows_attempts, windows_copy_store.rs:15-18: intent, assigned, committed,
    revoked or closed, :70-86), or None. One SELECT on an open_ro connection, closed at once; never a write or a PRAGMA."""
    with contextlib.closing(open_ro(db)) as connection:
        row = connection.execute('SELECT phase FROM runtime_windows_attempts WHERE nonce = ?', (nonce,)).fetchone()
    return None if row is None else row[0]


class PhaseWatch:
    """W1-AB (Ruling 147): every 250 ms, once nonce_of() names the run's nonce, its attempt phase read(nonce) from the owned
    accept DB ({'t', 'phase'}); an error is a row too ({'t', 'error': type name}) and the loop goes on."""

    def __init__(self, nonce_of, read, clock=time, interval=0.25):
        self.nonce_of, self.read, self.clock, self.interval = nonce_of, read, clock, interval
        self.rows = []
        self.stop = threading.Event()
        threading.Thread(target=self._loop, daemon=True).start()

    def _loop(self):
        while not self.stop.is_set():
            self.sample()
            self.stop.wait(self.interval)

    def sample(self):
        started = round(self.clock.monotonic(), 3)
        try:
            nonce = self.nonce_of()
            if nonce is not None:
                self.rows.append({'t': started, 'phase': self.read(nonce)})
        except Exception as error:
            self.rows.append({'t': started, 'error': type(error).__name__})

    def saw(self, phase):
        return any('phase' in row and row['phase'] == phase for row in list(self.rows))

    def result(self):
        self.stop.set()
        return list(self.rows)


def kept_before_close(phases):
    """The t of every error-free intent, assigned or committed row before the first closed row (by list order)."""
    first = next((i for i, row in enumerate(phases) if row.get('phase') == 'closed'), len(phases))
    return [row['t'] for row in phases[:first] if 'error' not in row and row.get('phase') in READER_KEPT]


def ga_verdict(states, phases, start_ref, emptied, deny, final, orphan=ORPHAN_SECONDS, bound=TIMEOUTS['closedAfterEnd'], need=20):
    """W1 (가) (Ruling 147; plan 10: the reader is kept while the orphan runs, then the receipt and the close), pure. With
    window_end = start_ref + orphan - 1: every state sample up to window_end is live; at least `need` error-free phase rows
    up to window_end, each intent, assigned or committed, one committed; one such row at or after window_end, before the
    first closed row (keptAtWindowEnd, review r2 F1); the receipt; the first closed phase within [window_end, start_ref +
    orphan + bound]; no deny marker; exit 0. A None start_ref fails every timed check. Attempt phases only move forward
    (windows_copy_store.rs: commit 382-395, revoke/seal 280-281 and 407-410, observed_closed 427-433; no delete), so that one
    row proves the reader was not closed at any time up to window_end, whatever happened to the samples before it."""
    final = final or {}
    end = None if start_ref is None else start_ref + orphan - 1
    run = [] if end is None else [row for row in states if row['t'] <= end]
    kept = [] if end is None else [row for row in phases if 'error' not in row and row['t'] <= end]
    closed = next((row['t'] for row in phases if row.get('phase') == 'closed'), None)
    verdict = {'keptRun': bool(run) and all(row['state'] in LIVE for row in run),
               'keptReader': (len(kept) >= need and all(row['phase'] in READER_KEPT for row in kept)
                              and any(row['phase'] == 'committed' for row in kept)),
               'keptAtWindowEnd': end is not None and any(t >= end for t in kept_before_close(phases)),
               'receipt': emptied is not None,
               'closedAfter': end is not None and closed is not None and end <= closed <= start_ref + orphan + bound,
               'noDenyMarker': deny is None,
               'exitedZero': final.get('state') == 'exited' and exit_code(final) == 0}
    verdict['pass'] = start_ref is not None and all(verdict.values())
    return verdict


def na_verdict(phases, emptied, deny, final):
    """W1 (나) (Ruling 147), pure: the receipt, a closed attempt phase, no deny marker, exit 0."""
    final = final or {}
    verdict = {'receipt': emptied is not None, 'closed': any(row.get('phase') == 'closed' for row in phases), 'noDenyMarker': deny is None,
               'exitedZero': final.get('state') == 'exited' and exit_code(final) == 0}
    verdict['pass'] = all(verdict.values())
    return verdict


def phase_evidence(rows):
    """Compact PhaseWatch evidence: the row and error counts, the first row of each phase change in order, and (review r2 F1)
    the t of the last kept row before the first closed and of the error rows (the first 100), so a masked stretch shows."""
    changes = []
    for row in rows:
        if 'phase' in row and (not changes or changes[-1]['phase'] != row['phase']):
            changes.append(row)
    kept, errors = kept_before_close(rows), [row['t'] for row in rows if 'error' in row]
    return {'phaseSamples': len(rows), 'phaseErrors': len(errors), 'phaseTransitions': changes,
            'lastKeptBeforeClose': kept[-1] if kept else None, 'phaseErrorTimes': errors[:100]}


class ConsoleSampler:
    """W1-H only: every 500 ms, one PowerShell each, visible console windows and the owned/probe process rows (booleans only).
    W1-F (a) uses ConsoleWatch instead."""

    def __init__(self, sample, clock=time, interval=0.5):
        self.sample, self.clock, self.interval = sample, clock, interval
        self.rows = []
        self.errors = 0
        self.stop = threading.Event()
        threading.Thread(target=self._loop, daemon=True).start()

    def _loop(self):
        while not self.stop.is_set():
            started = self.clock.monotonic()
            try:
                value = self.sample()
                self.rows.append({'t': round(started, 3), 'windows': value.get('windows') or [], 'processes': value.get('processes') or []})
            except Exception:
                self.errors += 1
            self.stop.wait(max(0.0, self.interval - (self.clock.monotonic() - started)))

    def result(self):
        self.stop.set()
        return {'samples': len(self.rows), 'errors': self.errors, 'rows': self.rows[-2000:]}


def console_analysis(samples, bound=WATCH['boundMs']):
    """I3: the achieved interval (largest gap by the watcher's own clock), skipped samples, and every console window that
    appeared after the first sample (the baseline, taken before any W1 run started), by window handle and PID."""
    pairs = list(zip(samples, samples[1:]))
    gaps = [round(b['ms'] - a['ms'], 1) for a, b in pairs]
    skipped = sum(1 for a, b in pairs if b['i'] != a['i'] + 1) + (1 if samples and samples[0]['i'] != 0 else 0)
    key = lambda w: (w.get('hwnd'), w.get('pid'))
    baseline = [dict(w) for w in samples[0]['windows']] if samples else []
    known, new = {key(w) for w in baseline}, []
    for row in samples[1:]:
        for window in row['windows']:
            if key(window) not in known:
                known.add(key(window))
                new.append(dict(window, firstMs=row['ms']))
    return {'samples': len(samples), 'achievedIntervalMs': max(gaps) if gaps else None, 'overBound': sum(1 for g in gaps if g > bound),
            'skipped': skipped, 'boundMs': bound, 'firstMs': samples[0]['ms'] if samples else None, 'lastMs': samples[-1]['ms'] if samples else None,
            'baseline': baseline[:100], 'newWindows': new[:200]}


def console_part(analysis):
    """W1-F (a). Any console window that appeared during the run fails it, whoever owns it (recorded in newWindows; the
    review's second option, instead of ancestry from the run's root PID). None appearing proves it only with at least
    two samples, no gap over the bound, no skipped or cut sample and the watcher still alive at the end; else None."""
    if analysis['newWindows']:
        return False
    if (analysis['samples'] < 2 or analysis['achievedIntervalMs'] is None or analysis['achievedIntervalMs'] > analysis['boundMs']
            or analysis['skipped'] or analysis.get('badLines') or not analysis.get('aliveAtEnd')):
        return None
    return True


class ConsoleWatch:
    """I3: W1-F's console windows from one long-lived watcher (win.console_watch) instead of one PowerShell per sample.
    The constructor waits for the first sample, the baseline, before any W1 run starts."""

    def __init__(self, process, first=WATCH['firstSample']):
        self.process, self.done = process, None
        try:
            process.read_json(0, first)
        except Exception:
            process.close()
            raise

    def result(self):
        if self.done is None:
            alive = self.process.child.poll() is None  # before close(), which ends it
            code = self.process.close()
            lines = list(self.process.lines)
            samples, bad = [], 0
            for line in lines:
                try:
                    value = json.loads(line)
                    if type(value.get('i')) is not int or not isinstance(value.get('ms'), (int, float)) or not isinstance(value.get('windows'), list):
                        raise ValueError(line[:80])
                    samples.append(value)
                except (ValueError, AttributeError):
                    bad += 1
            analysis = console_analysis(samples)
            analysis.update(aliveAtEnd=alive, badLines=bad, exit=code, stderr=redact(list(self.process.stderr)[-20:], tail=None))
            analysis['part'] = console_part(analysis)
            self.done = analysis
        return self.done

    def close(self):
        self.result()


def probe_starts(rows):
    """Distinct probe processes (pid + creation) with the time each was first sampled."""
    first = {}
    for row in rows:
        for proc in row['processes']:
            if proc.get('probe'):
                first.setdefault((proc.get('pid'), proc.get('creation')), row['t'])
    return sorted(first.values())


def spaced(starts, low=50, high=70, need=3):
    gaps = [round(b - a, 3) for a, b in zip(starts, starts[1:])]
    return len(starts) >= need and all(low <= g <= high for g in gaps[:need - 1]), gaps


class Background:
    def __init__(self, function):
        self.result = self.error = None
        self.done = threading.Event()
        threading.Thread(target=self._run, args=(function,), daemon=True).start()

    def _run(self, function):
        try:
            self.result = function()
        except Exception as error:
            self.error = {'type': type(error).__name__, 'code': getattr(error, 'code', None), 'message': str(error)[:600]}
        finally:
            self.done.set()


class W1:
    def __init__(self, rpc, load, record, evidence, win, answers, probe_marker, commit, clock=time):
        self.rpc, self.load, self.record_case, self.win, self.answers = rpc, load, record, win, answers
        self.probe_marker, self.commit, self.clock = probe_marker, commit, clock
        self.evidence = Path(evidence)
        self.cases = self.evidence / 'cases'
        self.cases.mkdir(mode=0o700, exist_ok=True)
        self.runs = {}
        self.project_id = None
        self.consoles = None
        self.console_error = None

    def finish(self, case, status, row, reason=None, partial=None):
        dump(self.cases / (case + '.json'), {'case': case, 'status': status, 'reason': reason, **row})
        self.record_case(case, status, ['cases/' + case + '.json'] + (['console-windows-samples.json'] if case == 'W1-F' else []), reason,
                         partial=partial, surface='installed-native')

    def prepare(self):
        m = self.load()
        known = (m.get('projectIdByName') or {}).get('P-WIN')
        self.project_id = known or self.rpc('projects.add', {'path': m['projectPaths']['P-WIN']})['id']
        return self.approve()

    def approve(self):
        """Item 5: the run context exactly as the app sends it (useRunActions.tsx:35) and the app's ensure() rule. W1 asks
        before the start, not on a trust error as the app's withTrust does, to record the run definitions (W1-E)."""
        return ensure_trust(self.rpc, trust_context(self.project_id, 'run'))

    def start(self, def_id):
        """runtime.start on a thread (it can await the holder); the run is discovered and recorded at once."""
        sent = self.clock.monotonic()
        call = Background(lambda: self.rpc('runtime.start', {'projectId': self.project_id, 'id': def_id}))
        deadline = sent + TIMEOUTS['start']
        run = None
        while run is None:
            items = self.rpc('runtime.runs', {'projectId': self.project_id, 'limit': 100}).get('items') or []
            known = {r['id'] for rows in self.runs.values() for r in rows}
            fresh = [r for r in items if r.get('defId') == def_id and r.get('id') not in known]
            if fresh:
                run = fresh[0]
            elif call.done.is_set() and call.error:
                return {'defId': def_id, 'sent': sent, 'call': call, 'run': None, 'error': call.error}
            elif self.clock.monotonic() >= deadline:
                raise TimeoutError('Owned run %s not visible within %d s' % (def_id, TIMEOUTS['start']))
            else:
                self.clock.sleep(0.1)
        self.runs.setdefault(def_id, []).append(run)
        return {'defId': def_id, 'sent': sent, 'call': call, 'run': run, 'runId': run['id']}

    def detail(self, run_id):
        return self.rpc('runtime.get', {'runId': run_id})['run']

    def watch(self, started):
        """Wait for windows.directory, then list its control dir every 100 ms."""
        deadline = started['sent'] + TIMEOUTS['assignedBound']
        while True:
            directory = control_dir(self.detail(started['runId']), self.load()['windowsRoot'])
            if directory is not None:
                return ControlWatch(directory, self.clock)
            if self.clock.monotonic() >= deadline:
                raise TimeoutError('runtime.get gave no owned Windows copy directory')
            self.clock.sleep(0.1)

    def until(self, run_id, done, timeout):
        rows, deadline = [], self.clock.monotonic() + timeout
        while True:
            detail = self.detail(run_id)
            rows.append({'t': round(self.clock.monotonic(), 3), 'state': detail.get('state'), 'exit': detail.get('exit')})
            if done(detail) or self.clock.monotonic() >= deadline:
                return detail, rows
            self.clock.sleep(0.2)

    # --- (가)(나) and (사) -------------------------------------------------------------------------
    def ab_part(self, def_id, db, timeout, sample_for=0):
        """One W1-AB part (Ruling 147): the run's states (every 500 ms for sample_for seconds after the send, then until it
        ends), its attempt phases (PhaseWatch) and, up to closedAfterEnd after it ends, its receipt and a closed phase. Only
        this run's nonce counts: every run of the owned copy shares the control dir."""
        started = self.start(def_id)
        watch = self.watch(started)
        run_id = started['runId']
        phases = PhaseWatch(lambda: watch.nonce_for(run_id), lambda nonce: read_phase(db, nonce), self.clock)
        try:
            running = []
            while self.clock.monotonic() - started['sent'] < sample_for:
                detail = self.detail(run_id)
                running.append({'t': round(self.clock.monotonic(), 3), 'state': detail.get('state')})
                self.clock.sleep(0.5)
            final, rows = self.until(run_id, lambda d: d.get('state') not in LIVE, timeout)
            deadline = self.clock.monotonic() + TIMEOUTS['closedAfterEnd']
            while (first_of(watch, 'emptied.json', watch.nonce_for(run_id)) is None or not phases.saw('closed')) and self.clock.monotonic() < deadline:
                self.clock.sleep(0.1)
        finally:
            phase_rows = phases.result()  # no DB reader outlives its part, also when the part raised
            watch.stop.set()
        nonce = watch.nonce_for(run_id)
        mine = lambda kind: first_of(watch, kind, nonce)
        return {'runId': run_id, 'sent': started['sent'], 'nonce': nonce, 'running': running, 'final': final, 'rows': rows, 'phases': phase_rows,
                'control': watch.result(started['sent']), 'startRef': mine('holder-ready') or mine('assigned.json'), 'assigned': mine('assigned.json'),
                'emptied': mine('emptied.json'), 'deny': mine('closed.json')}

    def ab_g(self, selected):
        """W1 (가)(나) and (사). Ruling 147: <nonce>.closed.json is the product's deny-execution marker (revoke_attempt,
        windows_observation.rs:302-319), never a committed attempt's close; that is the DB phase `closed`
        (store::observed_closed, :505-514), after the receipt (<nonce>.emptied.json)."""
        db = accept_db(self.load())
        a = self.ab_part('s7w1a', db, ORPHAN_SECONDS + TIMEOUTS['closedAfterEnd'] + 30, sample_for=ORPHAN_SECONDS - 3)
        ga = ga_verdict(a['running'] + a['rows'], a['phases'], a['startRef'], a['emptied'], a['deny'], a['final'])
        b = self.ab_part('s7w1b', db, 120)
        nb = na_verdict(b['phases'], b['emptied'], b['deny'], b['final'])
        row = {'ga': {'runId': a['runId'], 'whileRunning': a['running'], 'final': a['final'], 'states': a['rows'][-50:], 'control': a['control'],
                      'nonce': a['nonce'], 'verdict': ga, **phase_evidence(a['phases'])},
               'na': {'runId': b['runId'], 'final': b['final'], 'states': b['rows'][-50:], 'control': b['control'],
                      'nonce': b['nonce'], 'verdict': nb, **phase_evidence(b['phases'])},
               'parts': {'ga': ga['pass'], 'na': nb['pass']}}
        if 'W1-AB' in selected:
            failed = ['%s (%s)' % (part, ','.join(name for name, ok in verdict.items() if name != 'pass' and not ok))
                      for part, verdict in (('ga', ga), ('na', nb)) if not verdict['pass']]
            self.finish('W1-AB', 'FAIL' if failed else 'PASS', row, 'Failed parts: ' + ','.join(failed) if failed else None)
        if 'W1-G' in selected:
            delay = None if a['assigned'] is None else round(a['assigned'] - a['sent'], 3)
            ok = delay is not None and delay < TIMEOUTS['assigned']
            self.finish('W1-G', 'PASS' if ok else 'FAIL', {'assignedAfterSendSeconds': delay, 'runId': a['runId']},
                        None if ok else 'assigned.json did not appear within 35 s of the cold runtime.start send')
        return a['control']

    # --- (마) --------------------------------------------------------------------------------------
    def e(self, selected):
        first = self.start('s7w1e')
        detail, _ = self.until(first['runId'], lambda d: d.get('state') == 'running' or d.get('state') not in LIVE, 60)
        stop = self.rpc('runtime.stop', {'runId': first['runId']})
        self.commit(p_win_toml(W1_EDITED_SERVICE))
        trust = self.approve()
        second = self.start('s7w1e')
        detail2, _ = self.until(second['runId'], lambda d: d.get('state') == 'running' or d.get('state') not in LIVE, 60) if second.get('run') else (None, [])
        busy = (second.get('error') or {}).get('code')
        ok = bool(second.get('run')) and (detail2 or {}).get('state') == 'running' and not busy
        row = {'first': {'runId': first['runId'], 'state': detail.get('state'), 'stop': stop}, 'trust': trust,
               'second': {'runId': second.get('runId'), 'state': (detail2 or {}).get('state'), 'error': second.get('error')}}
        if 'W1-E' in selected:
            self.finish('W1-E', 'PASS' if ok else 'FAIL', row, None if ok else 'The second runtime.start did not return a running run')
        return second.get('runId')

    # --- (바) --------------------------------------------------------------------------------------
    def f(self, service_run):
        u = self.start('s7w1u')
        watch_u = self.watch(u)
        final_u, _ = self.until(u['runId'], lambda d: d.get('state') not in LIVE, 120)
        found = self.rpc('logs.search', {'sources': [{'kind': 'run', 'runId': u['runId']}], 'query': '한글 s7', 'regex': False, 'limit': 10})
        hits = found.get('items') or []
        exact = [h for h in hits if str(h.get('text', '')).rstrip('\r\n') == '한글 s7']
        x = self.start('s7w1x')
        final_x, _ = self.until(x['runId'], lambda d: d.get('state') not in LIVE, 120)
        stopped = None
        if service_run:
            self.rpc('runtime.stop', {'runId': service_run})
            stopped, _ = self.until(service_run, lambda d: d.get('state') not in LIVE, 60)
        files = watch_u.result(u['sent'])
        consoles = self.consoles.result() if self.consoles else {'samples': 0, 'part': None, 'newWindows': [],
                                                                  'error': self.console_error or 'The console watcher did not start'}
        dump(self.evidence / 'console-windows-samples.json', consoles)
        times = [log_epoch(h.get('ts')) for h in exact]
        first_line = min(times) if times and None not in times else None
        assigned = next((f['mtime'] for f in files['files'] if f['kind'] == 'assigned.json'), None)
        parts = {'a': consoles['part'],  # I3: console_part()
                 'b': bool(exact) and exit_code(final_x) == 7 and (stopped is None or stopped.get('state') == 'stopped') and assigned is not None,
                 # assigned.json before the first output line; not run when the log time is not ISO-8601
                 'order': None if first_line is None or assigned is None else assigned <= first_line}
        row = {'u': {'runId': u['runId'], 'final': final_u, 'hits': len(hits), 'exactHits': len(exact), 'control': files, 'firstLineEpoch': first_line},
               'x': {'runId': x['runId'], 'final': final_x}, 'serviceStopped': stopped, 'newConsoleWindows': consoles.get('newWindows'),
               'consoleSamples': consoles['samples'], 'achievedIntervalMs': consoles.get('achievedIntervalMs'), 'parts': parts}
        failed = [k for k, v in parts.items() if v is False]
        if failed:
            self.finish('W1-F', 'FAIL', row, 'Failed parts: ' + ','.join(failed))
        elif parts['a'] is None:
            self.finish('W1-F', 'NOT_RUN', row, 'W1-F (a) not proven: the console watcher did not keep the %d ms bound to the end (achieved %s ms, '
                        '%s skipped, alive at end %s%s)' % (WATCH['boundMs'], consoles.get('achievedIntervalMs'), consoles.get('skipped'),
                                                           consoles.get('aliveAtEnd'), '; ' + consoles['error'] if consoles.get('error') else ''),
                        partial=[k for k, v in parts.items() if v])
        elif parts['order'] is None:
            self.finish('W1-F', 'NOT_RUN', row, 'The log line time is not ISO-8601; assigned-before-output was not checked', partial=['a', 'b'])
        else:
            self.finish('W1-F', 'PASS', row)

    # --- (아) --------------------------------------------------------------------------------------
    def h(self):
        started = self.start('s7w1h')
        watch = self.watch(started)
        sampler = ConsoleSampler(lambda: self.win.console_windows(run_token(self.load()['windowsRoot']), self.probe_marker), self.clock)
        deadline = self.clock.monotonic() + TIMEOUTS['probeWatch']
        while self.clock.monotonic() < deadline:
            ok, _ = spaced(probe_starts(sampler.rows))
            if ok:
                break
            self.clock.sleep(1)
        holder = next(iter(watch.assigned.values()), None)
        row = {'runId': started['runId'], 'holder': holder}
        if holder is None:
            sampler.result()
            watch.result(started['sent'])
            self.finish('W1-H', 'FAIL', row, 'No holder identity in assigned.json')
            return
        stop = self.win.stop_exact('w1-holder', holder['pid'], holder['creationTime'])
        stopped_at = self.clock.monotonic()
        release = None
        while self.clock.monotonic() - stopped_at <= TIMEOUTS['releaseBound']:
            if watch.first('closed.json') is not None:
                release = round(watch.first('closed.json') - stopped_at, 3)
                break
            self.clock.sleep(0.1)
        samples = sampler.result()
        starts = probe_starts(samples['rows'])
        ok_a, gaps = spaced(starts)
        row.update(stop=stop, releaseSeconds=release, probeStarts=len(starts), probeGaps=gaps, control=watch.result(started['sent']),
                   final=self.detail(started['runId']).get('state'))
        parts = {'a': ok_a, 'b': release is not None and release <= TIMEOUTS['release']}
        row['parts'] = parts
        self.finish('W1-H', 'PASS' if all(parts.values()) else 'FAIL', row, None if all(parts.values()) else 'Failed parts: ' + ','.join(
            k for k, v in parts.items() if not v))

    def run(self, selected):
        require_lock()
        if 'W1-D' in selected:
            import evidence
            reason = evidence.ROW_TEXT['W1-D.default'] if self.answers.get('Q4') is not True else (
                'W1 (다): Q4 is yes, but the run may not install the .NET SDK (validate forbids installer commands) and this row '
                'has no reviewed helper; NOT_RUN.')
            self.record_case('W1-D', 'NOT_RUN', [], reason)
        wanted = [c for c in ('W1-AB', 'W1-G', 'W1-F', 'W1-E', 'W1-H') if c in selected]
        if not wanted:
            return
        self.prepare()
        if 'W1-F' in wanted:
            self.start_consoles()
        try:
            if {'W1-AB', 'W1-G'} & set(wanted):
                self.ab_g(selected)
            service = self.e(selected) if {'W1-E', 'W1-F'} & set(wanted) else None
            if 'W1-F' in wanted:
                self.f(service)
            elif service:
                self.rpc('runtime.stop', {'runId': service})
        finally:
            if self.consoles is not None:
                self.consoles.close()  # never left running past W1-F, also when an earlier row failed
        if 'W1-H' in wanted:
            self.h()

    def start_consoles(self):
        """I3: W1-F's one watcher, started once, before the first W1 run, and proven to sample (its baseline)."""
        self.consoles, self.console_error = None, None
        try:
            self.consoles = ConsoleWatch(self.win.console_watch(run_token(self.load()['windowsRoot']), WATCH['seconds']))
        except Exception as error:
            self.console_error = '%s: %s' % (type(error).__name__, redact_text(str(error), 300))
