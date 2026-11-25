"""Owned stage checks and failure-tolerant teardown (ported from the reviewed S4/S6 helper); operations are injected for pure tests.

S7: the stage root holds the marker and the subdirs baseline/ and candidate/ (changes §6); WSL-only
flows tear down only the WSL side (`sides`); the C5 accept-absent proof runs after every cleanup step.
"""
import copy
import json
from pathlib import Path
import re
import shutil
import time

# The only fingerprint subtree that may be reported outside the v1 prod comparison: the legacy v0.x
# catalog dir's integration snapshots (%LOCALAPPDATA%\Devbox\integration), rewritten by the user's running
# legacy apps (e.g. wsl-desktop). v1 product code at the bound SHA never writes there. FLAGGED for review.
LEGACY_PREFIX = re.compile(r'[A-Za-z]:\\Users\\[^\\]+\\AppData\\Local\\Devbox\\integration')


def split_legacy(windows, prefixes):
    """(v1 prod view, legacy external rows): only install-tree files under an exact legacy prefix move out."""
    prefixes = [str(p) for p in prefixes]
    if any(not LEGACY_PREFIX.fullmatch(p) for p in prefixes):
        raise ValueError('Legacy external prefix outside the reviewed scope: ' + repr(prefixes))
    lowered = [p.lower() + '\\' for p in prefixes]
    if not isinstance(windows, dict):
        return windows, []
    core = copy.deepcopy(windows)
    legacy = []
    for install in core.get('installs') or []:
        kept = []
        for row in install.get('files') or []:
            path = str(row.get('path', '')) if isinstance(row, dict) else ''
            (legacy if any(path.lower().startswith(p) for p in lowered) else kept).append(row)
        install['files'] = kept
    return core, sorted(legacy, key=lambda r: str(r.get('path', '')).lower())


def compare_prod(before, after, prefixes=()):
    """v1 prod (WSL + Windows minus the legacy subtree) must be byte-identical; legacy changes are reported."""
    core_before, legacy_before = split_legacy(before.get('windows'), prefixes)
    core_after, legacy_after = split_legacy(after.get('windows'), prefixes)
    live = prod_live(core_before, core_after)
    # 1.0.1: a difference in the running prod app or in the devbox:// command fails the comparison by itself, also when
    # one side is an older helper's fingerprint without those fields (not a proof).
    equal = before.get('wsl') == after.get('wsl') and core_before == core_after and not live.get('differenceKinds')
    rows_before = {str(r.get('path')).lower(): r for r in legacy_before}
    rows_after = {str(r.get('path')).lower(): r for r in legacy_after}
    changed = [{'path': (rows_before.get(k) or rows_after.get(k)).get('path'), 'before': rows_before.get(k), 'after': rows_after.get(k)}
               for k in sorted(set(rows_before) | set(rows_after)) if rows_before.get(k) != rows_after.get(k)]
    return {'prodUnchanged': equal, 'wslEqual': before.get('wsl') == after.get('wsl'), 'windowsV1Equal': core_before == core_after,
            **live,
            'legacyExternalWriterChanged': bool(changed),
            'legacyExternal': {'prefixes': list(prefixes), 'changed': changed, 'scope': 'legacy v0.x catalog integration snapshots; not v1 prod'}}


def prod_live(before, after):
    """1.0.1: the running prod app and the devbox:// command, named as their own difference kinds. Both are part of the
    Windows fingerprint, so either one already makes `prodUnchanged` false; these rows say which, with both sides.

    `prodProcessChanged`: the prod processes (PID and start time) differ. A prod app that was stopped or restarted shows
    as `gone`; one that appeared shows as `started`. The harness never stops or starts a prod app, so the controller
    judges whether the user did it; the run is a FAIL either way. A fingerprint from before 1.0.1 has no process list:
    then the comparison is not proven, which also counts as changed."""
    if not isinstance(before, dict) or not isinstance(after, dict):
        return {}
    key = lambda row: (row.get('pid'), row.get('startedUtc'), str(row.get('path', '')).lower())
    old, new = before.get('processes'), after.get('processes')
    known = isinstance(old, list) and isinstance(new, list)
    gone = [row for row in old if key(row) not in {key(r) for r in new}] if known else []
    started = [row for row in new if key(row) not in {key(r) for r in old}] if known else []
    command_known = 'protocolCommand' in before and 'protocolCommand' in after
    kinds = ([] if known and not gone and not started else ['prodProcessChanged']) + (
        [] if command_known and before['protocolCommand'] == after['protocolCommand'] else ['prodProtocolChanged'])
    return {'prodProcessChanged': 'prodProcessChanged' in kinds, 'prodProtocolChanged': 'prodProtocolChanged' in kinds, 'differenceKinds': kinds,
            'prodProcesses': {'known': known, 'before': old, 'after': new, 'gone': gone, 'started': started},
            'prodProtocolCommand': {'known': command_known, 'before': before.get('protocolCommand'), 'after': after.get('protocolCommand')}}


def stage_owner_matches(marker, token, sha):
    return isinstance(marker, dict) and set(marker) == {'token', 'sourceSha'} and marker.get('token') == token and marker.get('sourceSha') == sha


MARKER = '.s7-accept-owner.json'
STAGE_SUBDIRS = ('baseline', 'candidate')
# 1.0.1 (D5): the update flow's fixture folder in the stage root, with exactly these three files (update_flow.build_fixture).
UPDATE_SUBDIR = 'update'
UPDATE_FILES = ('owner.json', 'metadata.json', 'installer.exe')
# CS10's owned copy of the app settings sits in the stage root (changes §8.2).
STAGE_FILES = (MARKER, 'settings.json.s7-backup')


def cleanup_stage(directory, token, sha):
    """Remove the owned Windows installer stage root (marker match, only the known entries, no links)."""
    directory = Path(directory)
    try:
        if not directory.exists() and not directory.is_symlink():
            return {'status': 'PASS', 'directory': str(directory), 'removed': False, 'reason': 'already absent (uninstaller step removed it)'}
        if directory.is_symlink():
            raise ValueError('stage directory became a symlink')
        marker = json.loads((directory / MARKER).read_text(encoding='utf-8-sig'))
        if not stage_owner_matches(marker, token, sha):
            raise ValueError('stage owner mismatch')
        if any(p.is_symlink() for p in directory.rglob('*')):
            raise ValueError('stage contains links; retained')
        entries = {p.name: p for p in directory.iterdir()}
        unknown = sorted(n for n, p in entries.items() if not ((n in STAGE_SUBDIRS + (UPDATE_SUBDIR,) and p.is_dir()) or (n in STAGE_FILES and p.is_file())))
        if unknown:
            raise ValueError('stage holds unexpected entries; retained: ' + repr(unknown))
        update = entries.get(UPDATE_SUBDIR)
        if update is not None and any(not c.is_file() or c.name not in UPDATE_FILES for c in update.iterdir()):
            raise ValueError('stage subdir holds unexpected entries; retained: ' + UPDATE_SUBDIR)
        for side in STAGE_SUBDIRS:
            sub = entries.get(side)
            if sub is not None and any(not c.is_file() or not (c.name == 'receipt.json' or c.name.endswith('-setup.exe')) for c in sub.iterdir()):
                raise ValueError('stage subdir holds unexpected entries; retained: ' + side)
        shutil.rmtree(directory)
        if directory.exists() or directory.is_symlink():
            raise RuntimeError('stage directory remains')
        return {'status': 'PASS', 'directory': str(directory), 'removed': True, 'subdirs': sorted(n for n in entries if n in STAGE_SUBDIRS)}
    except Exception as error:
        return {'status': 'FAIL', 'directory': str(directory), 'removed': False, 'reason': str(error), 'retainedResources': [str(directory)]}


def finalize(outcome, context, before, operations, save):
    """Catch each operation separately, preserve uncertain resources, always persist final outcome.

    Only the final local evidence write can escape: a disk failure cannot honestly be reported as saved.
    Native interop, child timeout, fingerprint, or stage cleanup failure never skips that write.
    `context['sides']` is ('wsl', 'windows') for Windows flows and ('wsl',) for the WSL-only flows.
    """
    outcome.setdefault('cleanup', [])
    outcome.setdefault('retainedResources', [])
    outcome.setdefault('evidenceErrors', [])
    sides = tuple(context.get('sides', ('wsl', 'windows')))
    native = 'windows' in sides
    after = {}

    def write(name, value):
        try:
            save(name, value)
        except Exception as error:
            outcome['evidenceErrors'].append({'artifact': name, 'error': str(error)})

    def retain(resources):
        for resource in resources:
            if resource not in outcome['retainedResources']:
                outcome['retainedResources'].append(resource)

    def skip(name, reason, resources=()):
        outcome['cleanup'].append({'action': name, 'status': 'NOT_RUN', 'exit': None, 'reason': reason})
        retain(resources)

    def attempt(name, key, resources=()):
        try:
            value = operations[key]()
            failed = isinstance(value, dict) and (value.get('status') not in (None, 'PASS') or ('exit' in value and value['exit'] != 0))
            row = {'action': name, 'status': 'FAIL' if failed else 'PASS', 'exit': 1 if failed else 0, 'result': value}
            if failed:
                retain(resources)
                retain(value.get('retainedResources', []))
        except Exception as error:
            value = None
            row = {'action': name, 'status': 'FAIL', 'exit': None, 'error': str(error), 'errorType': type(error).__name__}
            retain(resources)
        outcome['cleanup'].append(row)
        write('teardown-' + name, row)
        return row['status'] == 'PASS', value
    try:
        retained = context.get('retainedResources', [])
        if native and context.get('launched'):
            attempt('quit', 'quit')
        else:
            skip('quit', 'No owned native launch recorded' if native else 'WSL-only flow: no native app')
        safe = True
        if native and context.get('installed'):
            safe, stopped = attempt('native-stopped-precondition', 'assert_stopped', retained)
            if safe and (not isinstance(stopped, dict) or stopped.get('stopped') is not True):
                safe = False
                outcome['cleanup'][-1].update(status='FAIL', exit=1, error='Native stopped proof absent')
                retain(retained)
        else:
            skip('native-stopped-precondition', 'No installation attempted' if native else 'WSL-only flow: no native app')
        if context.get('ownership') and safe:
            attempt('wsl', 'wsl_cleanup', context.get('wslResources', retained))
        else:
            skip('wsl', 'Missing ownership or native stop/identity is uncertain', retained)
        if native and context.get('installed') and safe:
            attempt('windows', 'windows_cleanup', context.get('windowsResources', retained))
        else:
            skip('windows', 'No installation, or native stop/identity is uncertain' if native else 'WSL-only flow: nothing installed on Windows', retained if native else ())
        # Read-only comparisons and owned stage cleanup are independent of stop failures.
        for side in sides:
            ok, value = attempt('prod-' + side + '-fingerprint', 'fingerprint_' + side)
            after[side] = {'status': 'PASS' if ok else 'NOT_PROVEN', 'value': value if ok else None}
        write('prod-after', after)
        known = all(after[x]['status'] == 'PASS' and before.get(x) is not None for x in sides)
        compared = compare_prod({x: before.get(x) for x in sides}, {x: after[x]['value'] for x in sides},
                                context.get('legacyExternalPrefixes', ())) if known else None
        equal = bool(compared and compared['prodUnchanged'])
        outcome['prodComparison'] = {'status': ('PASS' if equal else 'FAIL') if known else 'NOT_PROVEN', 'sides': list(sides),
                                     'reason': None if equal else ('Fingerprint changed' if known else 'Before/after fingerprint unavailable'),
                                     **(compared or {})}
        outcome['prodUnchanged'] = bool(equal) if known else None
        outcome['legacyExternalWriterChanged'] = compared['legacyExternalWriterChanged'] if known else None
        if context.get('staged') and not safe:
            # 1.0.1: no proven stop, no stage cleanup. A running owned app may hold the staged installer open, and a
            # removal that fails halfway would leave a stage that no longer matches its record. Retained whole: FAIL.
            why = 'Native stop/identity is uncertain; the installer stage is retained whole'
            skip('stage', why, [context.get('stageDirectory') or 'owned installer stage dir'])
            outcome['stageCleanup'] = {'status': 'NOT_RUN', 'removed': False, 'reason': why, 'retained': True}
        elif context.get('staged'):
            ok, value = attempt('stage', 'stage_cleanup', ['owned installer stage dir; inspect stageCleanup'])
            outcome['stageCleanup'] = value if value is not None else {'status': 'FAIL', 'removed': False, 'reason': 'Stage cleanup raised; retained'}
        else:
            skip('stage', 'Installer stage was not created')
            outcome['stageCleanup'] = {'status': 'NOT_RUN', 'removed': False, 'reason': 'Installer stage was not created'}
        # C5 (changes §5): after every cleanup step, a fresh accept-absent proof (native when interop was usable, and WSL).
        if 'absent_after' in operations:
            ok, value = attempt('accept-absent-after', 'absent_after')
            outcome['acceptAbsentAfter'] = value if ok else {'status': 'FAIL'}
        else:
            skip('accept-absent-after', 'No accept resource was staged')
    except Exception as error:
        # Protect the final record even if a bookkeeping assumption itself is wrong.
        outcome['cleanup'].append({'action': 'teardown-internal', 'status': 'FAIL', 'exit': None, 'error': str(error)})
        retain(context.get('retainedResources', []))
    finally:
        if (any(x.get('status') == 'FAIL' for x in outcome['cleanup']) or outcome['retainedResources'] or
                outcome.get('prodComparison', {}).get('status') != 'PASS' or outcome['evidenceErrors'] or
                outcome.get('status') == 'RUNNING'):
            outcome['status'] = 'FAIL'
        outcome.setdefault('prodComparison', {'status': 'NOT_PROVEN', 'reason': 'Teardown did not finish comparisons'})
        outcome['ended'] = time.time()
        save('driver-result', outcome)
    return outcome
