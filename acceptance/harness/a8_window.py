"""A8 (1.0.1, issue #678): the installed app's display polls follow its main window.

While its window shows, the app asks observe.system and observe.ports every 3 s (the status bar), and each observe.ports
makes the accept daemon start one `docker` child, `ps --format json` (crates/observe/src/docker_ports.rs). The host now
says when the main window is hidden or minimized (app/src-tauri/src/platform/presence.rs, app/src/shell/visibility.ts).
These two polls opt out of the interval gate in app/src/rpc/query.ts (refetchIntervalInBackground: true) and pause on
their own `enabled: visible` (app/src/features/runs/systemMetrics.ts:5-24, useSystemMetrics); they ask at once when the
window shows again (unit-tested in app/src/shell/visibility.test.tsx). A8 observes those host paths and these two polls;
the query.ts gate itself is covered by unit tests only (the same file).

Five phases, right after A2, on the window as A2 left it:
- V0 visible, 12 s from the agent's ready answer;
- H1 hidden: WM_CLOSE, which the product answers by hiding to its tray (lib.rs CloseRequested), 2 s settle, 12 s;
- S1 shown: a second plain start of the owned executable. The single-instance handler calls window::focus_main, the
  function the tray click calls (lib.rs, platform/tray.rs). 8 s from the command;
- M1 minimized: WM_SYSCOMMAND SC_MINIMIZE, 2 s settle, 12 s;
- R1 restored: WM_SYSCOMMAND SC_RESTORE, 8 s from the command. The window is left visible and restored.

A visible phase is observed from the moment its command was sent, a hidden one from its answer plus the settle; samples
in between belong to no phase. Two signals, read inside WSL from /proc only (no RPC during the case): the `docker`
children of the accept daemon, read every 20 ms, and the bytes the accept bridge processes moved (/proc/<pid>/io rchar
+ wchar), every 100 ms. The Windows side is one long-lived window agent, native-window.ps1: one command per stdin line,
one JSON line back. The phases' thresholds and the decision are in a8_decision.py.
"""
import json
import os
from pathlib import Path
import threading
import time

from a8_decision import (ACTION, IDLE_CHECK_SECONDS, IDLE_SECONDS, IDLE_WAIT_SECONDS, PLAN, SETTLE, THRESHOLDS, TIMEOUTS,
                         decide, ready_problem, step_problem, summarize)
from common import dump, redact_text
import update_flow

CASE = 'A8'
PROC = Path('/proc')
UNIT = 'devbox-accept@accept.service'
INTERVAL = 0.1  # the samples: bridge bytes and the daemon's counters
CHILD_INTERVAL = 0.02  # the daemon's children: a `ps --format json` of Docker's CLI took 30-40 ms here (2026-11-26)
SCAN_EVERY = 10  # the bridge processes are looked for every 10th sample (1 s); their counters are read every sample
CASE_SECONDS = 90.0
GATED = ('close', 'launch', 'minimize', 'restore')
CLOSE_SECONDS = 4.0  # the agent's exit once its stdin is closed
KILL_SECONDS = 5.0  # LineProcess.close: the kill and its wait when the agent did not exit by then
# Every wait has a bound, and the bounds keep the case within CASE_SECONDS plus the idle waits (at most IDLE_WAIT_SECONDS):
# the phases end by `last - state`, the state read and the repairs by `last` (both moved by the idle waits so far), and
# the end command and the agent's exit take at most END_RESERVE after it.
END_RESERVE = TIMEOUTS['end'] + CLOSE_SECONDS + KILL_SECONDS
AGENT_SECONDS = int(CASE_SECONDS + IDLE_WAIT_SECONDS)  # the agent answers no command after this (native-window.ps1 allows 30 to 240)
COMMANDS = ('close', 'minimize', 'restore', 'launch', 'idle', 'state', 'end')


# --- /proc, read-only ---------------------------------------------------------------------------------------------------
def parse_stat(text):
    """The fields of one /proc/<pid>/stat line that A8 reads. The command name may hold spaces and parentheses, so the
    fields are counted from the last ')' (proc(5): ppid 4, cminflt 11, cutime 16, cstime 17, starttime 22)."""
    head, close, rest = text.rpartition(')')
    pid, opened, comm = head.partition(' (')
    if not close or not opened:
        raise ValueError('Not a stat line')
    fields = rest.split()
    return {'pid': int(pid), 'comm': comm, 'ppid': int(fields[1]), 'cminflt': int(fields[8]), 'cticks': int(fields[13]) + int(fields[14]),
            'start': int(fields[19])}


def parse_io(text):
    """rchar + wchar of one /proc/<pid>/io: every byte the process moved through read and write calls."""
    rows = dict(line.split(':', 1) for line in text.splitlines() if ':' in line)
    return int(rows['rchar']) + int(rows['wchar'])


def children_of(proc, pid):
    """The PIDs of one process's children: its threads' `children` files (the kernel's own list), or, without them, every
    process whose stat names it as the parent. Raises when the process is gone."""
    found, files = set(), 0
    for task in (Path(proc) / str(pid) / 'task').iterdir():
        try:
            text = (task / 'children').read_text()
        except OSError:
            continue
        files += 1
        found.update(int(item) for item in text.split())
    if files:
        return found
    for entry in Path(proc).iterdir():
        if entry.name.isdigit():
            try:
                if parse_stat((entry / 'stat').read_text())['ppid'] == pid:
                    found.add(int(entry.name))
            except (OSError, ValueError, IndexError):
                continue
    return found


def child_row(proc, pid, parent):
    """((pid, start), comm, docker) of one child of `parent`, or None when it is gone or has another parent now. `docker` is
    whether its command name or its executable's name is `docker` (a child seen before its exec carries the daemon's)."""
    try:
        stat = parse_stat((Path(proc) / str(pid) / 'stat').read_text())
    except (OSError, ValueError, IndexError):
        return None
    if stat['ppid'] != parent:
        return None
    try:
        exe = os.path.basename(os.readlink(Path(proc) / str(pid) / 'exe'))
    except OSError:
        exe = None
    return (pid, stat['start']), stat['comm'], stat['comm'] == 'docker' or exe == 'docker'


def child_args(proc, pid):
    """At most four arguments of a `docker` child, each cut to 40 characters (observe.ports runs `ps --format json`)."""
    try:
        argv = (Path(proc) / str(pid) / 'cmdline').read_bytes().split(b'\0')
    except OSError:
        return None
    return [item.decode('utf-8', 'replace')[:40] for item in argv[1:5] if item]


def exe_id(proc, pid):
    meta = os.stat(Path(proc) / str(pid) / 'exe')
    return meta.st_dev, meta.st_ino


def docker_on_path(proc, pid):
    """Where the daemon's PATH finds `docker`, or None. Only that path is kept; the environment is read here and dropped."""
    try:
        environ = (Path(proc) / str(pid) / 'environ').read_bytes().split(b'\0')
    except OSError:
        return None
    path = next((item[5:] for item in environ if item.startswith(b'PATH=')), b'').decode('utf-8', 'replace')
    for folder in path.split(':'):
        candidate = os.path.join(folder, 'docker')
        if folder and os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


def daemon_identity(proof, unit, proc=PROC):
    """The accept daemon A2 proved (instance-proof-candidate.json) is still the running one, read without an RPC: the
    unit's MainPID and InvocationID are the proof's, and /proc/<pid>/exe is the same file as the proof's verified target."""
    pid = proof.get('runningPid')
    if proof.get('status') != 'PASS' or proof.get('phase') != 'candidate' or type(pid) is not int or pid <= 1:
        raise ValueError("A2's instance proof names no running candidate daemon")
    invocation = proof.get('invocationId')
    if unit.get('ActiveState') != 'active' or unit.get('MainPID') != str(pid) or not invocation or unit.get('InvocationID') != invocation:
        raise ValueError('The accept daemon is not the one A2 proved (state, MainPID or InvocationID changed)')
    target = proof.get('verifiedTarget')
    if not isinstance(target, str) or not os.path.isabs(target):
        raise ValueError("A2's instance proof names no verified binary")
    meta = os.stat(target)
    if exe_id(proc, pid) != (meta.st_dev, meta.st_ino):
        raise ValueError('The daemon process does not run the binary A2 proved')
    stat = parse_stat((Path(proc) / str(pid) / 'stat').read_text())
    return {'pid': pid, 'start': stat['start'], 'exe': [meta.st_dev, meta.st_ino], 'invocationId': invocation, 'dockerPath': docker_on_path(proc, pid)}


class Sampler:
    """Read-only, on two threads. Every CHILD_INTERVAL the daemon's children (a `ps` of Docker's CLI lives about 30-40 ms
    here, so 100 ms reads would miss most). Every INTERVAL a sample [t, `docker` children alive at the last read, bridge
    bytes since the previous sample, daemon cminflt, daemon cutime+cstime]; the daemon's last two count what its reaped
    children used, so they also move for a child that no read caught."""

    def __init__(self, daemon, proc=PROC, clock=time, interval=INTERVAL, scan_every=SCAN_EVERY, find=update_flow.accept_bridges,
                 child_interval=CHILD_INTERVAL):
        self.daemon, self.proc, self.clock, self.interval, self.scan_every, self.find = daemon, Path(proc), clock, interval, scan_every, find
        self.child_interval = child_interval
        self.rows, self.children, self.bridges, self.scans = [], {}, {}, []
        self.count, self.errors, self.first_error, self.changed = 0, 0, None, None
        self.alive, self.scan_errors, self.first_scan_error = 0, 0, None
        self.stop = threading.Event()
        self.threads = []

    def scan(self):
        """One read of the daemon's children: each one's first and last read, and whether it is (or became) `docker`."""
        t = self.clock.monotonic()
        pid = self.daemon['pid']
        if self.changed is not None:
            return
        try:
            if parse_stat((self.proc / str(pid) / 'stat').read_text())['start'] != self.daemon['start']:
                self.changed = self.changed or t  # another process under the daemon's PID: its children are not read
                return
            alive = 0
            for child in sorted(children_of(self.proc, pid)):
                found = child_row(self.proc, child, pid)
                if found is None:
                    continue
                key, comm, docker_child = found
                row = self.children.get(key)
                if row is None:
                    row = self.children[key] = {'pid': child, 'start': key[1], 'first': t, 'comm': comm, 'docker': False, 'args': None}
                row['last'] = t
                if docker_child and not row['docker']:
                    row.update(docker=True, comm=comm, args=child_args(self.proc, child))
                alive += row['docker']
        except Exception as error:  # the daemon went (the next sample records it), or an unreadable entry
            self.scan_errors += 1
            self.first_scan_error = self.first_scan_error or redact_text('%s: %s' % (type(error).__name__, error), 300)
            return
        # Only a complete read counts as one, so a failed read shows as a gap.
        self.alive = alive
        self.scans.append(t)

    def sample(self):
        t = self.clock.monotonic()
        try:
            self._sample(t)
        finally:
            self.count += 1

    def _sample(self, t):
        pid = self.daemon['pid']
        try:
            stat = parse_stat((self.proc / str(pid) / 'stat').read_text())
        except (OSError, ValueError, IndexError):
            stat = None
        if stat is None or stat['start'] != self.daemon['start']:
            if self.changed is None:
                self.changed = t
            return
        try:
            moved = self._bridges(t)
        except Exception as error:
            self.errors += 1
            self.first_error = self.first_error or redact_text('%s: %s' % (type(error).__name__, error), 300)
            return
        self.rows.append([t, self.alive, moved, stat['cminflt'], stat['cticks']])

    def _bridges(self, t):
        if self.count % self.scan_every == 0:
            for pid, start in sorted(self.find(self.proc)):
                key = (pid, int(start))
                if key in self.bridges:
                    continue
                try:
                    if exe_id(self.proc, pid) != tuple(self.daemon['exe']):
                        continue
                    total = parse_io((self.proc / str(pid) / 'io').read_text())
                except (OSError, ValueError, KeyError):
                    continue
                # Every bridge counts from the read that found it, so the bytes a reconnect moved before that (its
                # handshake and first snapshot) are not read as polling. One found after the first sample is a reconnect
                # or a new bridge (`late`).
                self.bridges[key] = {'pid': pid, 'start': key[1], 'first': t, 'last': t, 'base': total, 'total': total, 'gone': None,
                                     'late': self.count > 0}
        moved = 0
        for key, row in self.bridges.items():
            if row['gone'] is not None:
                continue
            try:
                if parse_stat((self.proc / str(row['pid']) / 'stat').read_text())['start'] != key[1]:
                    raise ProcessLookupError(row['pid'])
                total = parse_io((self.proc / str(row['pid']) / 'io').read_text())
            except (OSError, ValueError, IndexError, KeyError):
                row['gone'] = t
                continue
            moved += total - row['total']
            row['total'], row['last'] = total, t
        return moved

    def _every(self, work, interval):
        while not self.stop.is_set():
            began = self.clock.monotonic()
            work()
            self.stop.wait(max(0.0, interval - (self.clock.monotonic() - began)))

    def start(self):
        self.scan()
        self.threads = [threading.Thread(target=self._every, args=(self.scan, self.child_interval), daemon=True),
                        threading.Thread(target=self._every, args=(self.sample, self.interval), daemon=True)]
        for thread in self.threads:
            thread.start()
        return self

    def finish(self):
        self.stop.set()
        for thread in self.threads:
            thread.join(timeout=5)
        return self


# --- the run ------------------------------------------------------------------------------------------------------------
class Agent:
    """The window agent's stdin and stdout: one command, then its answer, matched by the command's number `n` (the agent
    counts the lines it read). A line that is not a JSON object with an `event` is kept as noise (bounded, redacted); an
    answer to another command (one that came after its wait ran out) is kept in `late` and skipped."""

    def __init__(self, process, clock=time):
        self.process, self.clock, self.cursor, self.noise, self.late, self.count = process, clock, 0, [], [], 0

    def reply(self, timeout, event, n=None):
        """The next answer whose `event` is `event` (and whose `n` is `n`, when given), or the agent's own `expired`."""
        deadline = self.clock.monotonic() + timeout
        while True:
            left = deadline - self.clock.monotonic()
            if left <= 0:
                raise TimeoutError('No window agent answer within %g s' % timeout)
            try:
                value = self.process.read_json(self.cursor, left)
            except ValueError:
                self.keep(self.process.lines[self.cursor])
                self.cursor += 1
                continue
            self.cursor += 1
            if not isinstance(value, dict) or 'event' not in value:
                self.keep(json.dumps(value))
                continue
            if value['event'] == 'expired' or (value['event'] == event and (n is None or value.get('n') == n)):
                return value
            if len(self.late) < 10:
                self.late.append({'event': value.get('event'), 'n': value.get('n'), 'ms': value.get('ms'), 'waitingFor': [event, n]})

    def keep(self, line):
        if len(self.noise) < 10:
            self.noise.append(redact_text(str(line), 200))

    def ask(self, command, timeout):
        """(sent, answer or None, answered): the monotonic times around one command and its answer."""
        if command not in COMMANDS:
            raise ValueError('Not a window agent command: %r' % (command,))
        self.count += 1
        number = self.count
        sent = self.clock.monotonic()
        try:
            self.process.send(command)
            answer = self.reply(timeout, command, number)
        except OSError:  # TimeoutError, or a closed pipe: no answer
            answer = None
        return sent, answer, self.clock.monotonic()


class Gate:
    """The user-idle gate (see IDLE_SECONDS). `wait` asks the agent's `idle` until the session had no keyboard or mouse input
    for IDLE_SECONDS, sleeping in between, within what is left of the case's IDLE_WAIT_SECONDS. `rows` keeps each wait with
    its checks [time, ms since the last input]; `waited` is the time spent in all, by which the case's deadlines move."""

    def __init__(self, agent, clock, t0):
        self.agent, self.clock, self.t0, self.waited, self.rows = agent, clock, t0, 0.0, []

    def wait(self, phase, command):
        """None when the window may change now, else {kind, why}: `userActive` when the time ran out with input in the session,
        `agent` when the agent gave no input time."""
        began = self.clock.monotonic()
        row = {'phase': phase, 'command': command, 'began': began, 'checks': [], 'passed': False}
        self.rows.append(row)
        try:
            while True:
                if IDLE_WAIT_SECONDS - self.waited - (self.clock.monotonic() - began) < TIMEOUTS['idle']:
                    return self.active(row, began)
                _, answer, answered = self.agent.ask('idle', TIMEOUTS['idle'])
                idle = answer.get('idleMs') if isinstance(answer, dict) and answer.get('event') == 'idle' else None
                row['checks'].append([answered, idle])
                if isinstance(idle, bool) or not isinstance(idle, (int, float)) or idle < 0:
                    return {'kind': 'agent', 'why': 'the window agent gave no time of the last user input before %s' % ACTION[command]}
                if idle >= IDLE_SECONDS * 1000:
                    row['passed'] = True
                    return None
                room = IDLE_WAIT_SECONDS - self.waited - (self.clock.monotonic() - began) - TIMEOUTS['idle']
                if room <= 0:
                    return self.active(row, began)
                self.clock.sleep(min(IDLE_SECONDS - idle / 1000.0, room, IDLE_CHECK_SECONDS))
        finally:
            row['ended'] = self.clock.monotonic()
            row['waited'] = round(row['ended'] - began, 3)
            self.waited += row['ended'] - began

    def active(self, row, began):
        inputs = [at - ms / 1000.0 for at, ms in row['checks'] if isinstance(ms, (int, float)) and not isinstance(ms, bool)]
        if not inputs:
            return {'kind': 'userActive', 'why': 'no time was left of the %g s A8 may wait for %g s without input' % (IDLE_WAIT_SECONDS, IDLE_SECONDS)}
        return {'kind': 'userActive', 'why': ('keyboard or mouse input in the session within %g s at each of %d checks over %g s (the last '
                                              '%g s after the case began); A8 waits at most %g s in all for %g s without input' % (
                                                  IDLE_SECONDS, len(inputs), round(self.clock.monotonic() - began, 1), round(max(inputs) - self.t0, 1),
                                                  IDLE_WAIT_SECONDS, IDLE_SECONDS))}


def sleep_until(clock, moment):
    left = moment - clock.monotonic()
    if left > 0:
        clock.sleep(left)


def acted_at(step):
    """The action's moment on the driver's clock: the agent's `actedMs` (right after the post, or after the second start
    ended) taken back from the answer's own `ms`, within [sent, answered]; `sent` when the answer has no such time."""
    reply = step['reply'] or {}
    try:
        at = step['answered'] - (float(reply['ms']) - float(reply['actedMs'])) / 1000.0
    except (KeyError, TypeError, ValueError):
        return step['sent']
    return min(max(at, step['sent']), step['answered'])


def drive(agent, clock, ready, deadline, steps, gate):
    """The five phases on the agent, appended to `steps` as {phase, kind, command, sent, answered, acted, reply, problem,
    observe}. They stop at the first problem. Each window change passes the gate first; `deadline` is the last moment a
    phase may end, moved later by the gate's waits."""
    v0 = clock.monotonic()
    first = {'phase': 'V0', 'kind': 'visible', 'command': None, 'sent': v0, 'answered': v0, 'reply': None, 'problem': None}
    steps.append(first)
    if v0 + PLAN[0][3] > deadline:
        first['problem'] = {'kind': 'deadline', 'why': 'the case would pass its %g s bound in V0' % CASE_SECONDS}
        return steps
    first['observe'] = [v0, v0 + PLAN[0][3]]
    sleep_until(clock, v0 + PLAN[0][3])
    for name, kind, command, seconds in PLAN[1:]:
        step = {'phase': name, 'kind': kind, 'command': command, 'sent': None, 'answered': None, 'reply': None, 'problem': None}
        steps.append(step)
        # The window ends at most the command's bound, the settle and its own length after the command is sent.
        need = TIMEOUTS[command] + (SETTLE if kind == 'hidden' else 0.0) + seconds
        for _ in range(3):  # input between the gate's check and the action makes the agent refuse: the gate waits again
            problem = gate.wait(name, command)
            if problem:
                step['problem'] = problem
                return steps
            if clock.monotonic() + need > deadline + gate.waited:
                step['problem'] = {'kind': 'deadline', 'why': 'the case would pass its %g s bound in %s' % (CASE_SECONDS, name)}
                return steps
            step['sent'], step['reply'], step['answered'] = agent.ask(command, TIMEOUTS[command])
            if not (isinstance(step['reply'], dict) and step['reply'].get('userActive')):
                break
        step['problem'] = step_problem(command, step['reply'], ready)
        if step['problem']:
            break
        step['acted'] = acted_at(step)
        start = step['answered'] + SETTLE if kind == 'hidden' else step['acted']
        step['observe'] = [start, start + seconds]
        sleep_until(clock, start + seconds)
    return steps


def left_of(state):
    """shown, hidden, minimized or gone: how a window state leaves the window for the rows after A8."""
    if not isinstance(state, dict) or not state.get('pid'):
        return 'gone'
    if not state.get('visible'):
        return 'hidden'
    return 'minimized' if state.get('iconic') else 'shown'


def restore(agent, clock, steps, last, gate):
    """Leave the window as A2 left it when the phases stopped early, by the product's own paths only: the second start for a
    hidden window, SC_RESTORE for a minimized one, each after the gate and within `last` (moved by the gate's waits). Never
    when the window or the app went, or a second app still runs. `left` says how the window was left."""
    final = {'repairs': []}
    _, answer, _ = agent.ask('state', TIMEOUTS['state'])
    final['state'] = answer
    kinds = {(s.get('problem') or {}).get('kind') for s in steps}
    if not isinstance(answer, dict) or answer.get('event') != 'state':
        final.update(skipped='the window agent did not answer state', left='unknown')
        return final
    state = answer.get('state') or {}
    if answer.get('why') or kinds & {'gone', 'secondInstance'}:
        final.update(skipped='the window or the app is gone, or a second app still runs', after=state,
                     left='gone' if answer.get('why') or 'gone' in kinds else left_of(state))
        return final

    def repair(command):
        problem = gate.wait('repair', command)
        if problem:
            final['skipped'] = '%s: %s' % (command, problem['why'])
            return state
        if clock.monotonic() + TIMEOUTS[command] > last + gate.waited:
            final['skipped'] = 'no time left for ' + command
            return state
        sent, reply, answered = agent.ask(command, TIMEOUTS[command])
        final['repairs'].append({'command': command, 'sent': sent, 'answered': answered, 'reply': reply})
        if isinstance(reply, dict) and reply.get('userActive'):
            final['skipped'] = '%s: input in the session right before it' % command
        return (reply or {}).get('after') or state
    if not state.get('visible'):
        state = repair('launch')
    if state.get('iconic') and 'skipped' not in final:
        state = repair('restore')
    final['after'] = state
    final['left'] = left_of(state)
    return final


def run(session, win, unit_state, clock=time, proc=PROC, sampler=Sampler):
    """A8 on the session right after A2. `win.window_agent(seconds)` starts the agent (a LineProcess); `unit_state(name)` is
    wsl-owned's read of a systemd unit."""
    t0 = clock.monotonic()
    row = {'case': CASE, 'startedWall': time.time(), 'plan': [list(p) for p in PLAN], 'settleSeconds': SETTLE, 'intervalSeconds': INTERVAL,
           'childIntervalSeconds': CHILD_INTERVAL, 'thresholds': THRESHOLDS, 'caseSeconds': CASE_SECONDS,
           'idle': {'seconds': IDLE_SECONDS, 'waitSeconds': IDLE_WAIT_SECONDS, 'checkSeconds': IDLE_CHECK_SECONDS}}
    a2 = session.matrix['cases']['A2']['status']
    ctx = {'a2': a2, 'ready': None, 'steps': [], 'phases': {}, 'dockerSeen': False, 'dockerPath': None, 'daemonChanged': None, 'left': None}
    if a2 != 'PASS':
        return conclude(session, row, ctx)
    try:
        launch = json.loads((session.evidence / 'native-launch-candidate.json').read_text())
        row['launch'] = {key: launch.get(key) for key in ('pid', 'executable', 'startedUtc')}
        proof = json.loads((session.evidence / 'instance-proof-candidate.json').read_text())
        row['daemon'] = daemon = daemon_identity(proof, unit_state(UNIT), proc)
        ctx['dockerPath'] = daemon['dockerPath']
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        ctx['ready'] = 'the A2 launch or its daemon could not be read: ' + redact_text(str(error), 300)
        return conclude(session, row, ctx)
    samples = sampler(daemon, proc=proc, clock=clock).start()
    agent = gate = None
    try:
        agent = Agent(win.window_agent(AGENT_SECONDS), clock)
        gate = Gate(agent, clock, t0)
        try:
            ready = agent.reply(TIMEOUTS['ready'], 'ready')
        except OSError as error:
            ready = {'event': None, 'error': redact_text(str(error), 300)}
        row['ready'] = dict(ready, answeredAt=round(clock.monotonic() - t0, 3))
        ctx['ready'] = ready_problem(ready, launch)
        last = t0 + CASE_SECONDS - END_RESERVE
        if not ctx['ready']:
            try:
                drive(agent, clock, ready, last - TIMEOUTS['state'], ctx['steps'], gate)
            finally:
                row['final'] = restore(agent, clock, ctx['steps'], last, gate)
                ctx['left'] = row['final'].get('left')
    finally:
        if agent is not None:
            try:
                row['end'] = agent.ask('end', TIMEOUTS['end'])[1]
            finally:
                row['agentExit'] = agent.process.close(CLOSE_SECONDS)
                row['agentNoise'] = agent.noise
                row['agentLate'] = agent.late
        samples.finish()
    children = list(samples.children.values())
    bridges = list(samples.bridges.values())
    for step in ctx['steps']:
        if step.get('observe') and not step.get('problem'):
            ctx['phases'][step['phase']] = summarize(step, samples.rows, children, bridges, samples.scans)
    ctx['dockerSeen'] = any(c['docker'] for c in children)
    ctx['daemonChanged'] = None if samples.changed is None else round(samples.changed - t0, 3)
    rel = lambda t: None if t is None else round(t - t0, 3)
    row['steps'] = [dict(s, sent=rel(s.get('sent')), answered=rel(s.get('answered')), acted=rel(s.get('acted')),
                         observe=[rel(t) for t in s['observe']] if s.get('observe') else None) for s in ctx['steps']]
    for repair in (row.get('final') or {}).get('repairs', []):
        repair.update(sent=rel(repair['sent']), answered=rel(repair['answered']))
    row['gates'] = [dict(g, began=rel(g['began']), ended=rel(g.get('ended')), checks=[[rel(at), ms] for at, ms in g['checks']],
                         lastInputs=[rel(at - ms / 1000.0) for at, ms in g['checks'] if isinstance(ms, (int, float)) and not isinstance(ms, bool)])
                    for g in (gate.rows if gate else [])]
    row['idleWaitedSeconds'] = round(gate.waited, 3) if gate else 0.0
    row['phases'] = ctx['phases']
    row['samples'] = {'columns': ['t', 'dockerAlive', 'bridgeBytes', 'daemonCminflt', 'daemonCticks'],
                      'rows': [[rel(r[0])] + r[1:] for r in samples.rows], 'count': samples.count, 'errors': samples.errors,
                      'firstError': samples.first_error, 'daemonChanged': ctx['daemonChanged']}
    row['childReads'] = {'intervalSeconds': CHILD_INTERVAL, 'count': len(samples.scans), 'errors': samples.scan_errors,
                         'firstError': samples.first_scan_error}
    row['dockerChildren'] = [dict(c, first=rel(c['first']), last=rel(c.get('last'))) for c in children if c['docker']]
    others = {}
    for c in children:
        if not c['docker']:
            others[c['comm']] = others.get(c['comm'], 0) + 1
    row['otherChildren'] = others
    row['bridges'] = [{'pid': b['pid'], 'start': b['start'], 'first': rel(b['first']), 'last': rel(b['last']), 'gone': rel(b['gone']),
                       'late': b.get('late'), 'baseBytes': b['base'], 'moved': b['total'] - b['base']} for b in bridges]
    return conclude(session, row, ctx)


def conclude(session, row, ctx):
    status, reason, details = decide(ctx)
    row.update(status=status, reason=reason, **details)
    dump(session.evidence / 'cases' / 'A8.json', row)
    compact = {name: {key: phase.get(key) for key in ('children', 'firstChildSeconds', 'bridgeRate')} for name, phase in ctx['phases'].items()}
    for name, share in details['bridgeShares'].items():
        compact.setdefault(name, {})['bridgeShare'] = share
    final = row.get('final') or {}
    session.case_result(CASE, status, ['cases/A8.json'], reason, surface='installed-native', mode=details['mode'], failKinds=details['failKinds'],
                        notRunKind=details['notRunKind'], phases=compact, finalWindow=final.get('after'), windowLeft=final.get('left'),
                        idleWaitedSeconds=row.get('idleWaitedSeconds'))
    return status
