"""Pure S7 acceptance accounting and exact artifact provenance checks (no host access).

The matrix sentences live in matrix-text.json, generated verbatim from s7-14-matrix.md. They are data,
never commands, and the file is part of helperSha256, so the approval covers every byte of them.
"""
import hashlib
import json
from pathlib import Path
import re
import time

BASE = Path(__file__).resolve().parent
REPO = 'jihoon22-lee/devbox'
# The default branch of each side's windows.yml dispatch (S8-5, Ruling 146: the candidate is built on main). 1.0.1 (D1): a
# run may name another branch with --candidate-branch/--baseline-branch; the approval binding pins it (`branch`).
BRANCHES = {'candidate': 'main', 'baseline': 'v1/s7-rc1-accept'}
BRANCH_NAME = re.compile(r'[A-Za-z0-9][A-Za-z0-9._/-]{0,127}')
# 1.0.1 (D1): the artifact folder of each side is a run argument (--candidate-artifacts/--baseline-artifacts), one path
# component under the evidence root. The approval binding pins the name (`artifacts`) next to the bytes (`artifactSha256`).
ARTIFACT_DIR_NAME = re.compile(r'[A-Za-z0-9][A-Za-z0-9._-]{0,127}')
ARTIFACT_DIR_RESERVED = ('runs', 'harness')
ARTIFACT_ENTRIES = ('devbox-setup', 'devbox-setup-accept', 'devbox-linux')
WORKFLOW = '.github/workflows/windows.yml'
SHA = re.compile('[0-9a-f]{40}')
TEXT = json.loads((BASE / 'matrix-text.json').read_text(encoding='utf-8'))
REASONS = TEXT['reasons']
ROW_TEXT = TEXT['rowText']
CASE_ROWS = TEXT['caseRows']
# The matrix's `no` rows, keyed by row ID, with their exact sentences.
NOT_RUN = TEXT['notRun']
STATUSES = ('PASS', 'FAIL', 'NOT_RUN')

# Case lists per flow (changes §8.0), in run order.
FLOWS = {
    'wsl-fresh': ['CS3', 'CS4', 'SV1', 'NT1', 'H3', 'H3X-CONFIG', 'H3X-ENV', 'H3X-REPORT', 'H3X-NEST', 'H3X-SHELL', 'H3X-EXIT', 'SN1',
                  'H4', 'K1X-SEND', 'H1X-TRUST', 'H2X-TRUST', 'H1', 'H1X-SHELL', 'H1X-HELP', 'P1', 'H2', 'H2X-RESUME', 'H2X-RESUME1S',
                  'H2X-FEEDBACK', 'H2X-SCREENS', 'H2X-SHELLMARK', 'P1X', 'SN2', 'CL1', 'CL2', 'CL3', 'C1', 'K1X-TMUXUP', 'M4'],
    'wsl-upgrade': ['U1', 'U1X', 'CS5'],
    # 1.0.1 (#678): A8 runs right after A2, on the window as A2 left it, and leaves it so.
    'fresh': ['G1', 'G2', 'A1', 'A2', 'A8', 'W1-AB', 'W1-G', 'W1-F', 'W1-E', 'W1-H', 'W1-D', 'A4', 'N1-ACCEPT', 'CS3', 'CS10', 'C1',
              'A6', 'A7', 'R-SC2', 'M4'],
    'upgrade': ['U1', 'U1X', 'CS5', 'R-SC2'],
    # 1.0.1 (D5): the app's own update from the baseline install to the candidate installer (UP1), the foreign files
    # across it (UP2) and across the uninstall (A7).
    'update': ['UP1', 'UP2', 'A7', 'M4'],
}
WSL_FLOWS = ('wsl-fresh', 'wsl-upgrade')
WINDOWS_FLOWS = ('fresh', 'upgrade', 'update')
# The flows with two sides: the baseline is installed first, the candidate over it.
UPGRADE_FLOWS = ('upgrade', 'wsl-upgrade', 'update')
# The update flow's version pair (update_flow.check_shape): `same-version` is what the accept update runner accepts.
UPDATE_SHAPES = ('same-version', 'next-version')
CASES = list(dict.fromkeys(c for flow in FLOWS.values() for c in flow))
# Case prerequisites inside one flow (changes §8.0). In the fresh flow every installed case needs the
# install (A1) and the launch (A2); the WSL flows start their daemon before any case.
_INSTALLED = {'A8', 'W1-AB', 'W1-G', 'W1-F', 'W1-E', 'W1-H', 'W1-D', 'A4', 'N1-ACCEPT', 'CS3', 'CS10', 'C1', 'R-SC2'}
NEEDS = {
    'fresh': {'A2': {'A1'}, 'A6': {'A1'}, 'A7': {'A1'}, 'M4': {'A1'}, **{case: {'A1', 'A2'} for case in _INSTALLED}},
    'upgrade': {},
    # The update flow always installs and starts the baseline; UP2 reads what the update of UP1 left.
    'update': {'UP2': {'UP1'}},
    'wsl-fresh': {},
    'wsl-upgrade': {},
}
# Cases that PASS only on the installed Windows app (changes §2, evidence.py:58-69).
REQUIRES_NATIVE = {'A1', 'A2', 'A4', 'A6', 'A7', 'A8', 'W1-AB', 'W1-G', 'W1-F', 'W1-E', 'W1-H', 'W1-D', 'CS10', 'N1-ACCEPT', 'R-SC2', 'UP1',
                   'UP2'}
# The user answers a PASS needs (matrix Q1, Q2, Q5); the gate is `trust_answer`.
TRUST_GATED = {'H1': ('Q1',), 'H1X-SHELL': ('Q1',), 'H1X-HELP': ('Q1',), 'P1': ('Q1',), 'H2': ('Q2',), 'H2X-RESUME': ('Q2',),
               'H2X-RESUME1S': ('Q2',), 'H2X-FEEDBACK': ('Q2',), 'H2X-SCREENS': ('Q2',), 'H2X-SHELLMARK': ('Q2',), 'P1X': ('Q2',),
               'SN2': ('Q1', 'Q2'), 'U1': ('Q1', 'Q2'), 'U1X': ('Q1', 'Q5'), 'CS5': ('Q1',)}
ANSWER_KEYS = ('Q0', 'Q1', 'Q2', 'Q3', 'Q4', 'Q5')
ISO = re.compile(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})')
TIMEOUT_TEXT = re.compile(r'timed out|시한을 초과')


class Unavailable(RuntimeError):
    """A precondition outside the harness's authority is missing: the case ends NOT_RUN with evidence."""


def reason(code):
    return REASONS[code]


def ui(what):
    """R-UI: the fixed prefix plus what the row needed."""
    return REASONS['R-UI-PREFIX'] + what


def parse_answers(data):
    """answers.json (changes §1.4): every key exact and typed; nothing else."""
    value = json.loads(data)
    required = {*ANSWER_KEYS, 'interopBroken', 'source', 'recordedBy', 'recordedAt'}
    if not isinstance(value, dict) or not required <= set(value) or set(value) - required - {'notes'}:
        raise ValueError('answers.json keys must be exactly Q0-Q5, interopBroken, source, recordedBy, recordedAt (+ notes)')
    if any(type(value[key]) is not bool for key in (*ANSWER_KEYS, 'interopBroken')):
        raise ValueError('answers.json answers and interopBroken must be booleans')
    if value['source'] != 'user' or value['recordedBy'] != 'controller' or not isinstance(value['recordedAt'], str) or not ISO.fullmatch(value['recordedAt']):
        raise ValueError('answers.json must be recorded by the controller from the user, with an ISO-8601 time')
    notes = value.get('notes', {})
    if not isinstance(notes, dict) or any(k not in ANSWER_KEYS or not isinstance(v, str) for k, v in notes.items()):
        raise ValueError('answers.json notes may only cite Q0-Q5')
    return value


def gates_for(flow, case):
    if flow in WSL_FLOWS:
        gates = ['installer', 'wsl_identity']
    elif case in ('G1', 'G2'):
        gates = []
    elif case in ('A1', 'A6', 'A7', 'M4'):
        gates = ['installer']
    elif case == 'A2':
        gates = ['installer', 'native_identity']
    else:
        gates = ['installer', 'native_identity', 'fixture']
    if case in ('U1', 'U1X', 'CS5'):
        gates.append('upgrade_identity')
    if case in TRUST_GATED:
        gates.append('trust_answer')
    return gates


def initial(binding, flow, cases, answers):
    """The run's matrix; both SHAs come from the source binding (changes §1.2)."""
    if flow not in FLOWS or any(c not in FLOWS[flow] for c in cases):
        raise ValueError('Unknown flow or case')
    pick = lambda side: None if not binding.get(side) else {'sha': binding[side]['sha'], 'version': binding[side]['version']}
    return {'flow': flow, 'candidate': pick('candidate'), 'baseline': pick('baseline'),
            'answers': {k: answers[k] for k in ANSWER_KEYS}, 'interopBroken': answers['interopBroken'],
            'selectedCases': list(cases),
            'cases': {key: {'status': 'NOT_RUN', 'reason': 'Not executed in this run', 'evidence': [], 'row': CASE_ROWS[key]}
                      for key in FLOWS[flow]},
            'notRun': {key: {'status': 'NOT_RUN', 'reason': text} for key, text in NOT_RUN.items()},
            'gates': {}, 'phases': [], 'cleanup': []}


def failure(step, why, stderr=None, journal=None, timeout=False, shell_init=None, message=None):
    """A failing step, as attribute() reads it. stderr/journal are scanned, never stored."""
    return {'step': str(step), 'reason': str(why)[:2000], 'stderr': stderr, 'journal': journal, 'timeout': bool(timeout),
            'shellInit': shell_init, 'message': message}


def attribute(matrix, case, fail):
    """Changes §1.6: under the recorded broken interop, a stall caused by the environment is NOT_RUN.

    Returns the reviewable original failure (counts only) when the FAIL becomes NOT_RUN, else None.
    """
    if not fail or matrix.get('interopBroken') is not True:
        return None
    stderr = fail.get('stderr') or ''
    if isinstance(stderr, bytes):
        stderr = stderr.decode('utf-8', 'replace')
    in_stderr = sum('UtilAcceptVsock' in line for line in stderr.splitlines())
    in_journal = sum('UtilAcceptVsock' in str(line) for line in (fail.get('journal') or []))
    message = str(fail.get('message') or '')
    shell_init = fail.get('shellInit')
    timed_out = bool(fail.get('timeout')) or bool(TIMEOUT_TEXT.search(message))
    # M14: a whole setup that timed out (its login-shell env capture can hang on interop) counts like the env step.
    stalled = (shell_init in ('setup-env', 'setup-timeout') and timed_out) or (
        shell_init == 'doctor-tool' and str(fail.get('step', '')).split(':')[-1].startswith('tool.') and bool(TIMEOUT_TEXT.search(message)))
    if not (in_stderr or in_journal or stalled):
        return None
    return {'status': 'FAIL', 'step': fail.get('step'), 'reason': fail.get('reason'), 'timeout': bool(fail.get('timeout')),
            'shellInit': shell_init, 'message': message[:200] or None, 'vsockStderrLines': in_stderr, 'vsockJournalLines': in_journal}


def record(matrix, case, status, evidence=(), reason=None, failure=None, partial=None, **details):
    if case not in matrix['cases'] or status not in STATUSES:
        raise ValueError('Unknown acceptance result')
    if status == 'FAIL':
        attributed = attribute(matrix, case, failure)
        if attributed:
            status, reason, details['originalFailure'] = 'NOT_RUN', REASONS['R-IOP-STALL'], attributed
    if matrix['cases'][case]['status'] == 'FAIL' and status != 'FAIL':
        return  # A recorded FAIL is never overwritten by a later PASS/NOT_RUN.
    if status == 'PASS':
        flow = matrix['flow']
        if not evidence or not all(matrix['gates'].get(key, {}).get('status') == 'PASS' for key in gates_for(flow, case)):
            raise ValueError('PASS requires the provenance/identity/fixture gates and evidence')
        if any(matrix['answers'].get(q) is not True for q in TRUST_GATED.get(case, ())):
            raise ValueError('PASS requires the recorded user answer (trust_answer)')
        if case not in ('G1', 'G2'):
            surface = details.get('surface')
            if surface not in ('installed-native', 'wsl-artifact'):
                raise ValueError('Installed acceptance cannot use browser/unit evidence')
            if (case in REQUIRES_NATIVE or flow in WINDOWS_FLOWS) and surface != 'installed-native':
                raise ValueError('This case passes only on the installed native app')
            if flow in WSL_FLOWS and surface != 'wsl-artifact':
                raise ValueError('A WSL-only flow records surface wsl-artifact')
    row = {'status': status, 'evidence': list(evidence), 'reason': reason, 'row': CASE_ROWS[case], **details}
    if partial is not None and status == 'NOT_RUN':
        row['partial'] = list(partial)
        row['display'] = 'NOT_RUN(부분: ' + ', '.join(partial) + ')'
    matrix['cases'][case] = row


def branch_name(value):
    """A bound branch name: plain characters only, no `..`, no leading or trailing `/`, no doubled `/`."""
    if (not isinstance(value, str) or not BRANCH_NAME.fullmatch(value) or '..' in value or '//' in value or value.endswith(('/', '.'))
            or value.endswith('.lock')):
        raise ValueError('Branch must be a plain branch name')
    return value


def verify_run(run, sha, expected_run, branch):
    """The exact windows.yml dispatch run of one artifact: its own bound branch, success only."""
    branch_name(branch)
    expected = (REPO, REPO, branch, WORKFLOW, sha, 'completed', 'success', 'workflow_dispatch', str(expected_run))
    actual = (run.get('repository', {}).get('full_name'), run.get('head_repository', {}).get('full_name'), run.get('head_branch'),
              run.get('path'), run.get('head_sha'), run.get('status'), run.get('conclusion'), run.get('event'), str(run.get('id')))
    if actual != expected:
        raise ValueError('Exact source/repository/branch/workflow/run status mismatch')


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


# Installer names come only from the receipt. Accept is the only installable channel; the prod
# receipt is read for provenance and its installer is never staged, copied or executed.
NAME_RULES = {'accept': ('Devbox Accept_', '-setup.exe'), 'prod': ('Devbox_', '-setup.exe')}


def verify_receipt(directory, sha, run_id, channel='accept', version=None):
    if channel not in NAME_RULES:
        raise ValueError('Unknown installer channel')
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError('Receipt directory must be a real directory')
    receipt = json.loads((directory / 'receipt.json').read_text(encoding='utf-8-sig'))
    if (receipt.get('repository'), receipt.get('workflow'), receipt.get('source_sha'), str(receipt.get('run_id')), receipt.get('channel')) != (REPO, WORKFLOW, sha, str(run_id), channel):
        raise ValueError('Receipt source/channel mismatch')
    installers = receipt.get('installers', {})
    if not isinstance(installers, dict) or len(installers) != 1 or not receipt.get('version'):
        raise ValueError('One NSIS installer and version required')
    if version is not None and receipt['version'] != version:
        raise ValueError('Receipt version differs from the bound source version')
    prefix, suffix = NAME_RULES[channel]
    for name, digest in installers.items():
        if Path(name).name != name or '/' in name or '\\' in name or not name.endswith(suffix) or not name.startswith(prefix):
            raise ValueError('Unsafe or wrong-channel installer filename')
        file = directory / name
        if file.is_symlink() or not file.is_file() or not re.fullmatch('[0-9a-fA-F]{64}', str(digest)) or sha256_file(file) != digest.lower():
            raise ValueError('Installer SHA256 mismatch')
    if {p.name for p in directory.glob('*.exe')} != set(installers):
        raise ValueError('Unexpected installer files')
    return receipt


def verify_linux_artifact(directory, source_sha, expected_version):
    """The exact three-file windows.yml devbox-linux artifact, without executing it."""
    directory = Path(directory)
    names = {'devbox-linux', 'linux-receipt.json', 'devbox-version.txt'}
    if directory.is_symlink() or not directory.is_dir() or {p.name for p in directory.iterdir()} != names:
        raise ValueError('Unexpected Linux artifact entries')
    if any((directory / name).is_symlink() or not (directory / name).is_file() for name in names):
        raise ValueError('Linux artifact entries must be regular files')
    proof = json.loads((directory / 'linux-receipt.json').read_text())
    if proof.get('source_sha') != source_sha or not re.fullmatch('[0-9a-f]{64}', proof.get('sha256', '')):
        raise ValueError('Linux artifact source/hash receipt mismatch')
    if (directory / 'devbox-version.txt').read_text() != expected_version:
        raise ValueError('Linux artifact version mismatch')
    if sha256_file(directory / 'devbox-linux') != proof['sha256']:
        raise ValueError('Linux artifact binary hash mismatch')
    return {**proof, 'version': expected_version}


def artifact_dir_name(value):
    """D1: one plain path component, never `.`/`..`, a separator, or one of the evidence root's own folders."""
    if (not isinstance(value, str) or not ARTIFACT_DIR_NAME.fullmatch(value) or '..' in value or value.endswith('.')
            or value in ARTIFACT_DIR_RESERVED):
        raise ValueError('Artifact folder must be one plain path component under the evidence root: ' + repr(value))
    return value


def artifact_dir(root, name):
    """D1: <evidence root>/<name>/artifacts for a bound folder name. The folder and its `artifacts` child must already
    exist as real directories (no symlink on either), directly under the evidence root."""
    root = Path(root)
    folder = root / artifact_dir_name(name)
    if folder.parent != root or folder.is_symlink() or not folder.is_dir():
        raise ValueError('Artifact folder must be an existing real directory under the evidence root: ' + name)
    artifacts = folder / 'artifacts'
    if artifacts.is_symlink() or not artifacts.is_dir():
        raise ValueError('Artifact folder must hold a real `artifacts` directory: ' + name)
    return artifacts


def _artifact_root(artifact_dir):
    root = Path(artifact_dir)
    if root.is_symlink() or not root.is_dir():
        raise ValueError('Artifact directory must be a real directory')
    entries = {p.name for p in root.iterdir()}
    if entries != set(ARTIFACT_ENTRIES):
        raise ValueError('Artifact directory must hold exactly devbox-setup, devbox-setup-accept and devbox-linux: ' + repr(sorted(entries)))
    return root


def verify_artifact_dir(artifact_dir, sha, run_id, version):
    """The controller-downloaded run: accept + prod receipts and the Linux artifact, one source, one version."""
    root = _artifact_root(artifact_dir)
    accept = verify_receipt(root / 'devbox-setup-accept', sha, run_id, channel='accept', version=version)
    prod = verify_receipt(root / 'devbox-setup', sha, run_id, channel='prod', version=version)
    linux = verify_linux_artifact(root / 'devbox-linux', sha, version)
    installer = next(iter(accept['installers']))
    return {'accept': accept, 'prod': prod, 'linux': linux, 'version': version, 'installerName': installer,
            'installerSha256': accept['installers'][installer].lower(),
            'installerPath': str(root / 'devbox-setup-accept' / installer), 'receiptPath': str(root / 'devbox-setup-accept' / 'receipt.json')}


def artifact_digest(artifact_dir):
    """sha256 over the sorted relative paths and file hashes of all three subdirs (every downloaded byte)."""
    root = _artifact_root(artifact_dir)
    rows = []
    for sub in ARTIFACT_ENTRIES:
        if (root / sub).is_symlink() or not (root / sub).is_dir():
            raise ValueError('Artifact subdirectory must be a real directory: ' + sub)
        for path in (root / sub).rglob('*'):
            if path.is_symlink():
                raise ValueError('Artifact directory holds a link: ' + str(path.relative_to(root)))
            if path.is_file():
                rows.append(path.relative_to(root).as_posix() + ':' + sha256_file(path))
    return hashlib.sha256('\n'.join(sorted(rows)).encode()).hexdigest()


# Code only: data the approval binds separately (answers, source binding, test binding) stays out,
# so a new candidate, run or answer changes only the approval's `binding` part (changes §1.3).
HELPER_DATA = ('matrix-text.json',)


def helper_hashes(base):
    base = Path(base)
    return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(base.iterdir())
            if p.is_file() and not p.is_symlink() and (p.suffix in {'.py', '.ps1'} or p.name in HELPER_DATA) and not p.name.startswith('test_')}


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def approval_binding(source_binding, flow, cases, answers, candidate, baseline, windows_local, update_shape=None, distro=None):
    """What the driver computes for the approval's `binding` part (changes §1.3). The update flow also binds its shape, and
    every flow the WSL distro the Windows app is pointed at (null for the WSL-only flows, which start no Windows program)."""
    if (flow in WINDOWS_FLOWS) != (distro is not None):
        raise ValueError('The distro is bound for the Windows flows only')
    if (flow == 'update') != (update_shape is not None) or (update_shape is not None and update_shape not in UPDATE_SHAPES):
        raise ValueError('The update shape is bound for the update flow only')
    # 1.0.1 (D1): the artifact folder name and the run's branch are run arguments, so the reviewer pins them here.
    side = lambda value: None if value is None else {'sha': value['sha'], 'runId': value['runId'], 'branch': branch_name(value['branch']),
                                                     'artifacts': artifact_dir_name(value['artifacts']), 'artifactSha256': value['artifactSha256']}
    return {'sourceBindingSha256': sha256_bytes(source_binding), 'flow': flow, 'cases': list(cases), 'answersSha256': sha256_bytes(answers),
            'windowsLocal': windows_local, 'distro': distro, 'candidate': side(candidate), 'baseline': side(baseline),
            **({'updateShape': update_shape} if update_shape is not None else {})}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False)


def verify_review(base, review_file, binding):
    """Two parts: helperSha256 (code bytes) and binding (run inputs). Each refuses with its own message."""
    review = json.loads(Path(review_file).read_text())
    if not isinstance(review, dict) or review.get('status') != 'APPROVE' or set(review) != {'status', 'helperSha256', 'binding'}:
        raise ValueError('Independent approval must be exactly {status: APPROVE, helperSha256, binding}')
    if canonical(review['helperSha256']) != canonical(helper_hashes(base)):
        raise ValueError('helperSha256: the approval does not match the current helper files')
    if canonical(review['binding']) != canonical(binding):
        raise ValueError('binding: the approval does not match this run (source binding, flow, cases, answers, SHA, run or artifacts)')
    return review


def parse_sc2(stderr_text):
    """Raw S6-4b SC2 lines from the owned app stderr. Values are recorded, not judged here. 1.0.1: the `start-phase` lines,
    one per finished start step on the clock of first-data-painted (app/src-tauri/src/core/paint_trace.rs start_phase_line),
    in log order; a start-phase line of another form is left out and counted."""
    first = [dict(run=m[0], elapsed_ms=int(m[1]), visible=m[2]) for m in re.findall(r'^first-data-painted run=(\S+) elapsed_ms=(\d+) visible=(\w+)\s*$', stderr_text, re.M)]
    palette = [dict(run=m[0], trace=int(m[1]), outcome=m[2], deferred=m[3], elapsed_ms=int(m[4]))
               for m in re.findall(r'^palette-painted run=(\S+) trace=(\d+) outcome=(\S+) deferred=(\w+) elapsed_ms=(\d+)\s*$', stderr_text, re.M)]
    ignored = re.findall(r'^palette-painted .* ignored\s*$', stderr_text, re.M)
    dropped = re.findall(r'^palette-trace-dropped.*$', stderr_text, re.M)
    lines = re.findall(r'^start-phase\b.*$', stderr_text, re.M)
    shaped = [re.fullmatch(r'start-phase run=(\S+) phase=(\S+) elapsed_ms=(\d+)\s*', line) for line in lines]
    phases = [dict(run=m[1], phase=m[2], elapsed_ms=int(m[3])) for m in shaped if m]
    return {'firstDataPainted': first, 'startPhases': phases, 'startPhasesIgnored': len(lines) - len(phases), 'palettePainted': palette,
            'paletteIgnored': len(ignored), 'paletteDropped': len(dropped)}


def wait_sc2(stderr_path, seconds=60, clock=time):
    """parse_sc2 of an owned launch's stderr log (its first MiB), read once a second until it holds a first-data-painted line
    or `seconds` have passed. `waitedSeconds` says how long; an empty firstDataPainted, that the bound ended the wait."""
    started = clock.monotonic()
    while True:
        text = stderr_path.read_text(errors='replace')[:1024 * 1024] if stderr_path.exists() else ''
        sc2 = parse_sc2(text)
        waited = clock.monotonic() - started
        if sc2['firstDataPainted'] or waited >= seconds:
            return {**sc2, 'waitedSeconds': round(waited, 1)}
        clock.sleep(1)
