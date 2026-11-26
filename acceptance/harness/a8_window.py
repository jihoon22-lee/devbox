"""A8 (1.0.1, issue #678): the installed app's display polls follow its main window.

While its window shows, the app asks observe.system and observe.ports every 3 s (app/src/features/runs/systemMetrics.ts,
the status bar), and each observe.ports makes the accept daemon start one `docker` child, `ps --format json`
(crates/observe/src/docker_ports.rs). The fix pauses those polls while the host says the window is hidden or minimized,
and asks at once when it shows again (app/src/rpc/query.ts, app/src/shell/visibility.ts, app/src-tauri/src/platform/presence.rs).

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
+ wchar), every 100 ms. The Windows side is one long-lived native-lifecycle.ps1 -Action window-agent: one command per
stdin line, one JSON line back.
"""
import json
import math
import os
from pathlib import Path
import threading
import time

from common import dump, redact_text
import update_flow

CASE = 'A8'
PROC = Path('/proc')
UNIT = 'devbox-accept@accept.service'
# (phase, kind, command, observed seconds), in run order.
PLAN = (('V0', 'visible', None, 12.0), ('H1', 'hidden', 'close', 12.0), ('S1', 'visible', 'launch', 8.0),
        ('M1', 'hidden', 'minimize', 12.0), ('R1', 'visible', 'restore', 8.0))
SETTLE = 2.0
INTERVAL = 0.1  # the samples: bridge bytes and the daemon's counters
CHILD_INTERVAL = 0.02  # the daemon's children: a `ps --format json` of Docker's CLI took 30-40 ms here (2026-11-26)
SCAN_EVERY = 10  # the bridge processes are looked for every 10th sample (1 s); their counters are read every sample
# A visible phase polls: >= 2 `docker` children in its window (V0 expects 4 at 3 s, S1 and R1 3 with the poll on show),
# and in S1 and R1 the first within 3 s of the action (the poll on show is at once; without it the interval restarts at
# 3 s). The action is the moment the agent posted SC_RESTORE, or the moment the second start ended (it hands over to the
# running app before it ends), both read on the agent's stopwatch. A hidden phase does not poll: no child after the
# settle, and the bridge moves at most 25 % of V0's bytes per second (the event stream stays). Inside a window the
# sampler may not leave more than 0.5 s between two samples (as W1-F), nor the child reads more than 0.2 s between two
# reads: a gap that long could hide a whole `docker` child. A first child missing in the 3 s after the action is not
# judged when the reads there left more than 30 ms (a child lives 30-40 ms), nor any shortfall of a shown phase whose
# bridge bytes show the polls: both are a sampling miss, not frozen data.
THRESHOLDS = {'visibleChildren': 2, 'firstChildSeconds': 3.0, 'hiddenChildren': 0, 'hiddenBridgeShare': 0.25, 'maxStepSeconds': 0.5,
              'maxChildGapSeconds': 0.2, 'firstChildGapSeconds': 0.03}
# Seconds. The agent's own waits inside them: 1.5 s for a posted message's effect; 5 s for the second start to end and
# 2 s for the window to show.
TIMEOUTS = {'ready': 20.0, 'close': 5.0, 'minimize': 5.0, 'restore': 5.0, 'state': 3.0, 'launch': 10.0, 'end': 3.0, 'idle': 3.0}
CASE_SECONDS = 90.0
# The user-idle gate: a window changes (H1, S1, M1, R1 and each repair) only after IDLE_SECONDS without keyboard or mouse
# input in the session. The second start may take the foreground (tao presses Alt when Windows refuses it), and minimize
# and restore move the focus, so keys the user types could land in the accept window. The case waits for that at most
# IDLE_WAIT_SECONDS in all, checking at least every IDLE_CHECK_SECONDS; then it stops (NOT_RUN user-active).
IDLE_SECONDS = 30.0
IDLE_WAIT_SECONDS = 120.0
IDLE_CHECK_SECONDS = 5.0
GATED = ('close', 'launch', 'minimize', 'restore')
CLOSE_SECONDS = 4.0  # the agent's exit once its stdin is closed
KILL_SECONDS = 5.0  # LineProcess.close: the kill and its wait when the agent did not exit by then
# Every wait has a bound, and the bounds keep the case within CASE_SECONDS plus the idle waits (at most IDLE_WAIT_SECONDS):
# the phases end by `last - state`, the state read and the repairs by `last` (both moved by the idle waits so far), and
# the end command and the agent's exit take at most END_RESERVE after it.
END_RESERVE = TIMEOUTS['end'] + CLOSE_SECONDS + KILL_SECONDS
AGENT_SECONDS = int(CASE_SECONDS + IDLE_WAIT_SECONDS)  # the agent answers no command after this (window-agent allows 30 to 240)
COMMANDS = ('close', 'minimize', 'restore', 'launch', 'idle', 'state', 'end')
ACTION = {'close': 'WM_CLOSE', 'minimize': 'SC_MINIMIZE', 'restore': 'SC_RESTORE', 'launch': 'the second start'}
WANTED = {'close': ('hide', lambda s: not s.get('visible')), 'launch': ('show', lambda s: bool(s.get('visible')) and not s.get('iconic')),
          'minimize': ('minimize', lambda s: bool(s.get('visible')) and bool(s.get('iconic'))),
          'restore': ('restore', lambda s: bool(s.get('visible')) and not s.get('iconic'))}
FROZEN, LEAK = 'visible-without-polling', 'hidden-with-polling'
COMMAND_OF = {phase: command for phase, _, command, _ in PLAN}


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


# --- the phases: pure ---------------------------------------------------------------------------------------------------
def max_step(times, start, end):
    """The longest time without a sample around [start, end): from the last sample before it to the first after it. The
    whole span when no sample fell inside."""
    inside = [t for t in times if start <= t < end]
    if not inside:
        return round(end - start, 3)
    before = [t for t in times if t < start][-1:] or [start]
    after = [t for t in times if t >= end][:1] or [end]
    points = before + inside + after
    return round(max(b - a for a, b in zip(points, points[1:])), 3)


def summarize(step, rows, children, bridges, scans=()):
    """One phase's signals in its observed window [start, end): the `docker` children first seen there (with the first after
    the command, for S1 and R1), the bridge bytes in all and per second, the accept bridges alive, the daemon's
    reaped-children counters, and the longest step of the samples and of the child reads. Times in the result are relative
    to the window's start."""
    start, end = step['observe']
    seconds = end - start
    inside = [r for r in rows if start <= r[0] < end]
    polled = sorted((c for c in children if c['docker'] and start <= c['first'] < end), key=lambda c: c['first'])
    moved = sum(r[2] for r in inside)
    bins = [0] * max(1, math.ceil(seconds - 1e-9))
    for r in inside:
        bins[min(int(r[0] - start), len(bins) - 1)] += r[2]
    first = gap = None
    if step['kind'] == 'visible' and step.get('command'):
        # The window of a shown phase starts at its action (see THRESHOLDS).
        after = [c['first'] for c in children if c['docker'] and start <= c['first'] < end]
        first = round(min(after) - start, 3) if after else None
        gap = max_step(list(scans), start, start + THRESHOLDS['firstChildSeconds'])
    edge = [r for r in rows if r[0] < start][-1:] + inside
    reaped = {'cminflt': edge[-1][3] - edge[0][3], 'cticks': edge[-1][4] - edge[0][4]} if len(edge) > 1 else None
    alive = [b for b in bridges if b['first'] < end and (b['gone'] is None or b['gone'] >= start)]
    return {'phase': step['phase'], 'kind': step['kind'], 'seconds': round(seconds, 3), 'children': len(polled),
            'childTimes': [round(c['first'] - start, 3) for c in polled], 'childArgs': sorted({' '.join(c['args'] or []) for c in polled}),
            'firstChildSeconds': first, 'bridgeBytes': moved, 'bridgeRate': round(moved / seconds, 1) if seconds > 0 else 0.0,
            'bridgePerSecond': bins, 'bridges': len(alive), 'reaped': reaped, 'samples': len(inside),
            'maxStepSeconds': max_step([r[0] for r in rows], start, end), 'childReads': sum(1 for s in scans if start <= s < end),
            'maxChildGapSeconds': max_step(list(scans), start, end), 'firstReadGapSeconds': gap,
            'bridgeReconnects': sum(1 for b in bridges if b.get('late') and start <= b['first'] < end)}


def ready_problem(ready, launch):
    """Why the agent's ready answer does not prove A2's owned app and its main window as A2 left it, or None. `launch` is
    A2's own launch record (native-launch-candidate.json)."""
    if not isinstance(ready, dict) or ready.get('event') != 'ready':
        return 'the window agent did not answer ready'
    if ready.get('ok') is not True:
        return 'the window agent found no owned main window: ' + redact_text(str(ready.get('reason')), 300)
    if type(ready.get('pid')) is not int or ready.get('pid') != launch.get('pid') or ready.get('startedUtc') != launch.get('startedUtc'):
        return "the agent's app is not A2's launch (PID or start time differ)"
    if str(ready.get('executable') or '').lower() != str(launch.get('executable') or '-').lower():
        return "the agent's app runs another executable than A2's launch"
    if type(ready.get('hwnd')) is not int or ready['hwnd'] <= 0 or ready.get('class') != 'Tauri Window' or ready.get('title') != 'devbox':
        return 'the agent named no main window (Tauri Window, devbox)'
    state = ready.get('state') or {}
    if state.get('pid') != ready['pid'] or state.get('visible') is not True or state.get('iconic') is not False:
        return 'the main window is not visible and restored as A2 left it'
    if type(ready.get('trayWindows')) is not int or ready['trayWindows'] < 1:
        return 'the owned app has no tray window, so WM_CLOSE would quit it (lib.rs hides to the tray only while the tray exists)'
    return None


def step_problem(command, reply, ready):
    """Why one command did not do what its phase needs, as {kind, why}, or None. Kinds: `gone` (the window or the app went:
    FAIL), `secondInstance` (the second start did not hand over and end: FAIL), `userActive` (input in the session right
    before the action, so the agent did not act), `refused`, `post`, `state` and `agent` (NOT_RUN: nothing to judge)."""
    action = ACTION[command]
    if not isinstance(reply, dict):
        return {'kind': 'agent', 'why': 'the window agent did not answer %s within %g s' % (command, TIMEOUTS[command])}
    if reply.get('event') != command:
        return {'kind': 'agent', 'why': 'the window agent answered %r to %s' % (reply.get('event'), command)}
    if reply.get('error'):
        return {'kind': 'agent', 'why': 'the window agent failed %s: %s' % (command, redact_text(str(reply['error']), 300))}
    if reply.get('gone'):
        return {'kind': 'gone', 'why': redact_text(str(reply.get('why')), 300)}
    if reply.get('refused'):
        return {'kind': 'refused', 'why': redact_text(str(reply['refused']), 300)}
    if reply.get('userActive'):
        return {'kind': 'userActive', 'why': 'input in the session %s ms before %s, each time the gate had seen %g s without input' % (
            reply.get('idleMs'), action, IDLE_SECONDS)}
    if command == 'launch':
        second = reply.get('second') or {}
        if reply.get('started') is not True:
            return {'kind': 'agent', 'why': 'the second start was not made'}
        if second.get('exited') is not True:
            return {'kind': 'secondInstance', 'why': 'the second start still ran after 5 s instead of handing over to the running app '
                                                     '(PID %s, recorded; teardown stops it)' % second.get('pid')}
        if second.get('exitCode') != 0:
            return {'kind': 'secondInstance', 'why': 'the second start ended with exit code %s' % second.get('exitCode')}
    elif reply.get('posted') is not True:
        return {'kind': 'post', 'why': '%s was not posted (Win32 error %s)' % (action, reply.get('postError'))}
    after = reply.get('after') or {}
    if after.get('pid') != ready['pid']:
        return {'kind': 'gone', 'why': 'after %s the handle is no longer the owned main window' % action}
    name, wanted = WANTED[command]
    if not wanted(after):
        return {'kind': 'state', 'why': 'the window did not %s after %s (visible=%s, iconic=%s)' % (name, action, after.get('visible'), after.get('iconic'))}
    return None


def sampling_miss(name, command, row, phases, reference, problems):
    """(kind, line) when a shown phase's `docker` shortfall may be the child reads' and not the app's, else None (NOT_RUN,
    not FAIL). `sampling-miss`: the bridge bytes carry the polls (S1 and R1: more than 25 % of V0's rate, with a second
    above that within 3 s of the action; V0, the reference: every hidden phase observed moved at most 25 % of it).
    `sampling-gap`: at most one child short, and the reads in the 3 s after the action left more than 30 ms (about a
    `docker` child's life), so the first child could be missed."""
    t = THRESHOLDS
    said = ', '.join(problems)
    if reference:
        floor = t['hiddenBridgeShare'] * reference
        if name == 'V0':
            # Only when V0 saw a child at all: bytes alone (A2's start-up loads) must not excuse a V0 that never polled.
            hidden = [phases[p[0]] for p in PLAN if p[1] == 'hidden' and p[0] in phases]
            if row['children'] >= 1 and hidden and all(h['bridgeRate'] <= floor for h in hidden):
                return 'sampling-miss', ('sampling-miss in V0: %s, but every hidden phase moved at most %d%% of its bridge bytes, so V0 '
                                         'polled and the child reads missed its children' % (said, t['hiddenBridgeShare'] * 100))
        else:
            early = next((k for k, moved in enumerate(row['bridgePerSecond']) if moved > floor), None)
            if row['bridgeRate'] > floor and early is not None and early < t['firstChildSeconds']:
                return 'sampling-miss', ('sampling-miss in %s: %s, but its bridge carried the polls (%g B/s, %s of V0, from second %d '
                                         'after %s), so the child reads missed them' % (
                                             name, said, row['bridgeRate'], percent(row['bridgeRate'] / reference), early, ACTION[command]))
    gap = row.get('firstReadGapSeconds')
    if command and row['children'] + 1 >= t['visibleChildren'] and gap is not None and gap > t['firstChildGapSeconds']:
        return 'sampling-gap', ('sampling-gap in %s: %s, and the child reads left %g s without a read within %g s of %s (more than '
                                '%g s), so the first child could be missed' % (
                                    name, said, gap, t['firstChildSeconds'], ACTION[command], t['firstChildGapSeconds']))
    return None


def judge(phases, mode, bridged):
    """(FAIL lines, FAIL kinds, each phase's bridge share of V0, sampling misses [(kind, line)]) of the phases observed to
    their end. A visible phase that did not poll is the frozen-data failure; a hidden phase that did is #678 not fixed; a
    shown phase whose shortfall the reads may have caused is a sampling miss (NOT_RUN)."""
    t = THRESHOLDS
    v0 = phases.get('V0')
    reference = v0['bridgeRate'] if bridged and v0 else None
    lines, kinds, shares, misses = [], [], {}, []
    for name, kind, command, seconds in PLAN:
        row = phases.get(name)
        if row is None:
            continue
        share = round(row['bridgeRate'] / reference, 3) if reference else None
        shares[name] = share
        problems = []
        if kind == 'visible' and mode == 'docker':
            if row['children'] < t['visibleChildren']:
                problems.append('%d `docker` children in %g s (needs >= %d)' % (row['children'], row['seconds'], t['visibleChildren']))
            if command and row['firstChildSeconds'] is None:
                problems.append('no `docker` child after %s (needs one within %g s)' % (ACTION[command], t['firstChildSeconds']))
            elif command and row['firstChildSeconds'] > t['firstChildSeconds']:
                problems.append('first `docker` child %g s after %s (needs <= %g s)' % (row['firstChildSeconds'], ACTION[command], t['firstChildSeconds']))
        elif kind == 'visible' and mode == 'bridge' and command:
            floor = t['hiddenBridgeShare'] * reference
            active = next((k for k, moved in enumerate(row['bridgePerSecond']) if moved > floor), None)
            if share is None or share <= t['hiddenBridgeShare']:
                problems.append('bridge %g B/s, %s of V0 (needs more than %d%%)' % (row['bridgeRate'], percent(share), t['hiddenBridgeShare'] * 100))
            if active is None or active >= t['firstChildSeconds']:
                problems.append('no second above %d%% of V0 within %g s of %s' % (t['hiddenBridgeShare'] * 100, t['firstChildSeconds'], ACTION[command]))
        elif kind == 'hidden':
            if mode == 'docker' and row['children'] > t['hiddenChildren']:
                problems.append('%d `docker` children after the %g s settle (needs %d)' % (row['children'], SETTLE, t['hiddenChildren']))
            if share is not None and share > t['hiddenBridgeShare']:
                problems.append('bridge %g B/s, %s of V0 (needs <= %d%%)' % (row['bridgeRate'], percent(share), t['hiddenBridgeShare'] * 100))
        if not problems:
            continue
        if kind == 'visible' and mode == 'docker':
            miss = sampling_miss(name, command, row, phases, reference, problems)
            if miss:
                misses.append(miss)
                continue
        if kind == 'visible':
            kinds.append(FROZEN)
            reaped = row.get('reaped')
            # The daemon's reaped-children counters move for a child that no read caught too.
            seen = '; the daemon reaped children worth %d minor faults there' % reaped['cminflt'] if reaped else ''
            if name == 'V0' and reference:
                # A V0 that did not poll is no reference for the hidden phases' bytes: they are judged by their children.
                reference = None
                seen += "; V0's bridge bytes are then no reference for the hidden phases"
            lines.append('Visible window did not poll in %s (frozen data): %s%s' % (name, ', '.join(problems), seen))
        else:
            kinds.append(LEAK)
            lines.append('Hidden window still polled in %s (#678 not fixed): %s' % (name, ', '.join(problems)))
    return lines, list(dict.fromkeys(kinds)), shares, misses


def percent(share):
    return 'n/a' if share is None else '%d%%' % round(share * 100)


def decide(ctx):
    """(status, reason, details) of A8. Pure. ctx: `a2` (A2's status), `ready` (None, or why the app, its window or its daemon
    was not proven), `steps` (each phase's {phase, problem}), `phases` (summaries of the phases observed to their end),
    `dockerSeen` (a `docker` child of the daemon was seen at any time), `dockerPath`, `daemonChanged` (seconds or None),
    `left` (how the window was left: shown, hidden, minimized, gone or unknown). details: `mode`, `failKinds`,
    `bridgeShares`, and `notRunKind` (user-active, sampling-gap, sampling-miss, ... for a NOT_RUN)."""
    details = {'mode': None, 'failKinds': [], 'bridgeShares': {}, 'notRunKind': None}
    if ctx['a2'] != 'PASS':
        details['notRunKind'] = 'needs-a2'
        return 'NOT_RUN', 'A8 needs A2 (the owned launch that painted first data); A2 is %s' % ctx['a2'], details
    if ctx['ready']:
        details['notRunKind'] = 'precondition'
        return 'NOT_RUN', 'Precondition: ' + ctx['ready'], details
    problems = [(s['phase'], s['problem']) for s in ctx['steps'] if s.get('problem')]
    fatal = next(((phase, p) for phase, p in problems if p['kind'] in ('gone', 'secondInstance')), None)
    phases = ctx['phases']
    health = None
    if ctx['daemonChanged'] is not None:
        health = 'daemon-changed', 'The accept daemon changed during A8 (%g s after the start): its children could not be attributed' % ctx['daemonChanged']
    else:
        gap = max(((row['maxStepSeconds'], name) for name, row in phases.items()), default=(0, None))
        reads = max(((row['maxChildGapSeconds'], name) for name, row in phases.items()), default=(0, None))
        if gap[0] > THRESHOLDS['maxStepSeconds']:
            health = 'sampler-gap', 'The WSL sampler left %g s without a sample in %s (more than %g s), so its bridge bytes are incomplete' % (
                gap[0], gap[1], THRESHOLDS['maxStepSeconds'])
        elif reads[0] > THRESHOLDS['maxChildGapSeconds']:
            health = 'sampler-gap', 'The WSL child reads left %g s without a read in %s (more than %g s), so a short `docker` child could be missed' % (
                reads[0], reads[1], THRESHOLDS['maxChildGapSeconds'])
    v0 = phases.get('V0')
    bridged = bool(v0 and v0['bridges'] and v0['bridgeBytes'] > 0)
    mode = 'docker' if ctx['dockerSeen'] else 'bridge' if bridged else None
    details['mode'] = mode
    lines, kinds, shares, misses = judge(phases, mode, bridged) if mode and not health else ([], [], {}, [])
    details.update(failKinds=kinds, bridgeShares=shares)
    left = ctx.get('left')
    note = '; the window was left %s' % left if left in ('hidden', 'minimized') else ''
    if fatal:
        details['failKinds'] = ['window-gone' if fatal[1]['kind'] == 'gone' else 'second-instance'] + kinds
        head = ('The owned main window went at %s (%s): the product quit or closed it; teardown handles the rest' % (fatal[0], fatal[1]['why'])
                if fatal[1]['kind'] == 'gone' else '%s: %s' % (fatal[0], fatal[1]['why']))
        return 'FAIL', '; '.join([head] + lines) + note, details
    if health:
        details['notRunKind'] = health[0]
        return 'NOT_RUN', health[1] + note, details
    stop = problems[0] if problems else None
    if lines:
        tail = ['the case stopped at %s: %s' % (stop[0], stop[1]['why'])] if stop else []
        return 'FAIL', '; '.join(lines + [line for _, line in misses] + tail) + note, details
    reasons = []
    if stop:
        phase, problem = stop
        if problem['kind'] == 'userActive':
            reasons.append('user-active before %s (%s): %s, so the phases from there on were not judged' % (
                phase, ACTION.get(COMMAND_OF.get(phase), phase), problem['why']))
        else:
            reasons.append('%s: %s, so the phases from there on were not judged' % (phase, problem['why']))
        details['notRunKind'] = 'user-active' if problem['kind'] == 'userActive' else problem['kind']
    reasons += [line for _, line in misses]
    if reasons:
        details['notRunKind'] = details['notRunKind'] or misses[0][0]
        return 'NOT_RUN', '; '.join(reasons) + note, details
    if mode is None:
        details['notRunKind'] = 'no-signal'
        return 'NOT_RUN', ('Neither signal worked: no `docker` child of the accept daemon at any time (on the daemon PATH: %s) and no '
                           'accept bridge traffic in V0' % (ctx['dockerPath'] or 'none')), details
    if mode == 'bridge':
        return 'PASS', ('Decided from the bridge bytes only: no `docker` child of the accept daemon at any time (on the daemon PATH: %s). '
                        'V0 is the reference, so a V0 that never polled would show as a hidden phase that is not quiet' % (ctx['dockerPath'] or 'none')), details
    if not bridged:
        return 'PASS', 'The bridge bytes were not available (no accept bridge traffic in V0): the hidden phases were decided from the `docker` children only', details
    return 'PASS', None, details


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
