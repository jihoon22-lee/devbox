"""A8 (1.0.1, issue #678): the phases, their thresholds and the decision. Pure: no I/O; a8_window.py observes the
phases, drives the window agent and records the case."""
import math

from common import redact_text

# (phase, kind, command, observed seconds), in run order.
PLAN = (('V0', 'visible', None, 12.0), ('H1', 'hidden', 'close', 12.0), ('S1', 'visible', 'launch', 8.0),
        ('M1', 'hidden', 'minimize', 12.0), ('R1', 'visible', 'restore', 8.0))
SETTLE = 2.0
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
# The user-idle gate: a window changes (H1, S1, M1, R1 and each repair) only after IDLE_SECONDS without keyboard or mouse
# input in the session. The second start may take the foreground (tao presses Alt when Windows refuses it), and minimize
# and restore move the focus, so keys the user types could land in the accept window. The case waits for that at most
# IDLE_WAIT_SECONDS in all, checking at least every IDLE_CHECK_SECONDS; then it stops (NOT_RUN user-active).
IDLE_SECONDS = 30.0
IDLE_WAIT_SECONDS = 120.0
IDLE_CHECK_SECONDS = 5.0
ACTION = {'close': 'WM_CLOSE', 'minimize': 'SC_MINIMIZE', 'restore': 'SC_RESTORE', 'launch': 'the second start'}
WANTED = {'close': ('hide', lambda s: not s.get('visible')), 'launch': ('show', lambda s: bool(s.get('visible')) and not s.get('iconic')),
          'minimize': ('minimize', lambda s: bool(s.get('visible')) and bool(s.get('iconic'))),
          'restore': ('restore', lambda s: bool(s.get('visible')) and not s.get('iconic'))}
FROZEN, LEAK = 'visible-without-polling', 'hidden-with-polling'
COMMAND_OF = {phase: command for phase, _, command, _ in PLAN}


# --- the phases ---------------------------------------------------------------------------------------------------------
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
