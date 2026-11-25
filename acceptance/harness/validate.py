#!/usr/bin/env python3
"""S7-14 static/pure validation (changes §13). Never stages, installs, launches or creates WSL/Windows resources.

Checks: the forbidden strings, the tool-config path scan, RPC/tmux/MCP allowlist consistency, WSL purity,
the scratch path, the pure fixture plans for both modes, the .ps1 encoding and launch rules (1.0.1), and (only
with --powershell-parse, which needs interop) the Windows PowerShell parser over every .ps1. Without that flag
the parse is NOT_RUN.
Exits 0 with no source binding (UNBOUND); prints one JSON report.
"""
import argparse
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

from common import BASE, PS, dump, probe_script, redact_text, run_checked, verification_root
from evidence import helper_hashes
from fixture import BINDING_FILE, fixture_plan, fixture_root, source_config, validate_plan, verify_source

# S6 validate.py:18-20 plus the changes §13 additions. A raw substring scan of each .py/.ps1 helper (tests
# and this file excluded), and the same scan over the space-joined constant items of every list/tuple literal.
FORBIDDEN = ['DevboxAccept-s4', 'devbox-s4-accept', '.s4-accept-owner', 'v1/s4-editor-git', 'S4OwnedProcessLauncher', 'S4Clipboard',
             'Set-ExecutionPolicy', '-ExecutionPolicy', 'SetForegroundWindow', 'AllowSetForegroundWindow', 'AttachThreadInput', 'netsh',
             'New-NetFirewallRule', 'docker ', 'wsl.exe --shutdown', 'wsl --shutdown', "[Environment]::SetEnvironmentVariable('", 'setx ',
             'gh run download', 'gh workflow run',
             'SendInput', 'keybd_event', 'mouse_event', 'PostMessage', 'SendMessage', 'gh pr ', 'gh release', 'mcp add', 'git push',
             'git config --global', 'systemctl --user disable', 'systemctl --user kill', 'apt ', 'winget', 'npm install', 'pip install']
SCRATCH = '/tmp/devbox-s7-acceptance-'
TOOL_PATHS = ('.claude.json', '.codex/config.toml', '~/.claude', '.config/opencode', '~/.pi', '.bashrc')
# Path parts that name the same tool configs when joined with a home directory.
TOOL_PARTS = ('.claude', '.codex', '.pi', '.config/opencode', '.bashrc')
TRUST_PROBE = ('agents_live.py', 'trust_probe')
WRITES = {'write_text', 'write_bytes', 'unlink', 'rmtree', 'rename', 'replace', 'touch', 'symlink_to', 'chmod', 'mkdir', 'copyfile', 'copy', 'move'}
TMUX_SUBCOMMANDS = {'capture-pane', 'has-session', 'list-buffers', 'display-message', 'show-options',
                    'send-keys', 'set-buffer', 'paste-buffer', 'resize-window', 'delete-buffer'}
MCP_TOOLS = {'devbox_agents', 'devbox_agent_send'}
RPC_METHOD = re.compile(r'[a-z_]+\.[a-z_]+')
PURE_WSL = ('wsl_daemon.py', 'agents_live.py', 'daemon_rows.py')
NATIVE_NAMES = {'ps_command', 'PS', 'native', 'run_probe', 'stop_exact', 'identity_states', 'LineProcess', 'native_class_source',
                'interop_probe', 'IDENTITY_TYPE', 'STOP_TYPE', 'NativeOps', 'windows_flow', 'native_teardown', 'native_absent',
                'native_command', 'console_watch', 'console_windows'}
NATIVE_MODULES = {'a4_windows_service', 'w1_windows', 'g1_live_member', 'g2_probe_timing', 'update_flow'}
UTF8_BOM = b'\xef\xbb\xbf'
NATIVE_TEXT = re.compile(r'(?i)\.ps1\b|powershell\.exe|/mnt/c/Windows')
# The driver's WSL-only paths (changes §13): reachable from these Session methods, with `if native_flow:`
# branches pruned, nothing may name native code.
WSL_ROOTS = ('wsl_flow', 'teardown', 'close_mcp')
MANIFEST_EXCLUDED = re.compile(r'(README\.md|test_.*|review-approval.*|helper-manifest\.json|.*\.tmp)')


def helpers(base=BASE):
    """Runtime helper sources (.py/.ps1), tests and this validator excluded."""
    return {p.name: p.read_text(encoding='utf-8-sig') for p in sorted(Path(base).iterdir())
            if p.is_file() and p.suffix in {'.py', '.ps1'} and not p.name.startswith('test_') and p.name != 'validate.py'}


def constants(node):
    """Every str constant under node (f-string parts included), with its line."""
    for item in ast.walk(node):
        if isinstance(item, ast.Constant) and isinstance(item.value, str):
            yield item.lineno, item.value


def joined_lists(tree):
    """Space-joined constant items of each list/tuple literal (catches ['git', 'push'] forms)."""
    for item in ast.walk(tree):
        if isinstance(item, (ast.List, ast.Tuple)):
            words = [e.value for e in item.elts if isinstance(e, ast.Constant) and isinstance(e.value, str)]
            if len(words) > 1:
                yield item.lineno, ' '.join(words)


def scan_forbidden(sources):
    findings = []
    for name, text in sources.items():
        for needle in FORBIDDEN + [SCRATCH]:
            if needle in text:
                findings.append('%s: %s' % (name, needle))
        if name.endswith('.py'):
            for line, words in joined_lists(ast.parse(text)):
                findings += ['%s:%d: %s (list form)' % (name, line, n) for n in FORBIDDEN if n in words]
    return findings


def scan_gh(sources):
    """The only gh use is the read-only `gh api repos/...` (changes §2, driver.py)."""
    findings = []
    for name, text in sources.items():
        if not name.endswith('.py'):
            continue
        for item in ast.walk(ast.parse(text)):
            if isinstance(item, ast.List) and item.elts:
                head = item.elts[0]
                is_gh = (isinstance(head, ast.Name) and head.id == 'GH') or (isinstance(head, ast.Constant) and head.value in ('gh', '/snap/bin/gh'))
                if not is_gh:
                    continue
                second = item.elts[1] if len(item.elts) > 1 else None
                third = item.elts[2] if len(item.elts) > 2 else None
                ok = (isinstance(second, ast.Constant) and second.value == 'api' and isinstance(third, ast.JoinedStr)
                      and isinstance(third.values[0], ast.Constant) and str(third.values[0].value).startswith('repos/'))
                if not ok or len(item.elts) != 3:
                    findings.append('%s:%d: gh other than the read-only `gh api repos/...`' % (name, item.lineno))
    return findings


def scan_ps1_encoding(base=BASE):
    """1.0.1 (D4): Windows PowerShell 5.1 reads a .ps1 without a BOM in the ANSI code page (CP949 on the acceptance PC), so
    a non-ASCII character in such a file silently becomes other characters. Every helper .ps1 must be pure ASCII, or
    start with the UTF-8 BOM and be valid UTF-8."""
    findings = []
    for path in sorted(Path(base).glob('*.ps1')):
        data = path.read_bytes()
        if data.startswith(UTF8_BOM):
            try:
                data[len(UTF8_BOM):].decode('utf-8')
            except UnicodeDecodeError as error:
                findings.append('%s: has a UTF-8 BOM but is not valid UTF-8 (byte %d)' % (path.name, error.start + len(UTF8_BOM)))
            continue
        offset = next((i for i, byte in enumerate(data) if byte >= 0x80), None)
        if offset is not None:
            findings.append('%s:%d: non-ASCII byte 0x%02x in a .ps1 without a UTF-8 BOM' % (path.name, data.count(b'\n', 0, offset) + 1, data[offset]))
    return findings


def scan_ps1_launch(sources):
    """1.0.1 (D3): no helper starts a process with redirected output through Start-Process. That call creates the child
    with every inheritable handle of the PowerShell process, the WSL interop pipes included, so the calling WSL command
    does not return before the child exits. The owned executable starts only through process-launch.ps1's handle list."""
    findings = []
    for name, text in sources.items():
        if not name.endswith('.ps1'):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            code = line.split('#', 1)[0]
            if 'Start-Process' in code and ('-Redirect' in code or '-NoNewWindow' in code or '-UseNewEnvironment' in code):
                findings.append('%s:%d: Start-Process that hands this shell\'s handles to the child' % (name, number))
    return findings


def _inside(node, function):
    return function.lineno <= node.lineno <= function.end_lineno


def scan_tool_paths(sources):
    """Tool-config names occur only inside agents_live.trust_probe, which opens them 'rb' and writes nothing."""
    findings = []
    for name, text in sources.items():
        if name.endswith('.ps1'):
            findings += ['%s: %s' % (name, needle) for needle in TOOL_PATHS if needle in text]
            continue
        tree = ast.parse(text)
        probe = next((f for f in ast.walk(tree) if isinstance(f, ast.FunctionDef) and (name, f.name) == TRUST_PROBE), None)
        for item in ast.walk(tree):
            if not (isinstance(item, ast.Constant) and isinstance(item.value, str)):
                continue
            value = item.value
            hit = any(n in value for n in TOOL_PATHS) or value.strip('/') in TOOL_PARTS or value.startswith(tuple(p + '/' for p in TOOL_PARTS))
            if hit and not (probe and _inside(item, probe)):
                findings.append('%s:%d: tool config name outside trust_probe' % (name, item.lineno))
        if probe is not None:
            for call in ast.walk(probe):
                if not isinstance(call, ast.Call):
                    continue
                func = call.func
                if isinstance(func, ast.Name) and func.id == 'open':
                    mode = call.args[1] if len(call.args) > 1 else next((k.value for k in call.keywords if k.arg == 'mode'), None)
                    if not (isinstance(mode, ast.Constant) and mode.value == 'rb'):
                        findings.append('%s:%d: trust_probe opens a file other than rb' % (name, call.lineno))
                if isinstance(func, ast.Attribute) and func.attr in WRITES:
                    findings.append('%s:%d: trust_probe calls %s' % (name, call.lineno, func.attr))
    return findings


def callee(call):
    func = call.func
    return func.id if isinstance(func, ast.Name) else func.attr if isinstance(func, ast.Attribute) else None


def rpc_methods(sources):
    """(file, line, method) for every constant dotted method passed to rpc(), rpc_owned() or an injected call()."""
    rows = []
    for name, text in sources.items():
        if not name.endswith('.py'):
            continue
        for call in ast.walk(ast.parse(text)):
            if isinstance(call, ast.Call) and callee(call) in ('rpc', 'rpc_owned', 'call'):
                for arg in call.args[:2]:
                    if isinstance(arg, ast.Constant) and isinstance(arg.value, str) and RPC_METHOD.fullmatch(arg.value):
                        rows.append((name, call.lineno, arg.value))
                        break
    return rows


def scan_allowlists(sources, allowed_rpc, tmux_allowed=TMUX_SUBCOMMANDS, mcp_allowed=MCP_TOOLS):
    findings = ['%s:%d: RPC method %s not in READ_ONLY/OWNED_MUTATIONS' % row for row in rpc_methods(sources) if row[2] not in allowed_rpc]
    for name, text in sources.items():
        if not name.endswith('.py'):
            continue
        tree = ast.parse(text)
        for line, value in constants(tree):
            if value in ('/usr/bin/tmux', 'tmux') and name != 'agents_live.py':
                findings.append('%s:%d: tmux outside agents_live.Pane' % (name, line))
        for item in ast.walk(tree):
            if isinstance(item, ast.List) and item.elts and isinstance(item.elts[0], ast.Constant):
                head = item.elts[0].value
                if name == 'agents_live.py' and isinstance(head, str) and re.fullmatch(r'[a-z]+(-[a-z]+)+', head) and head not in tmux_allowed \
                        and any(_inside(item, c) for c in ast.walk(tree) if isinstance(c, ast.ClassDef) and c.name == 'Pane'):
                    findings.append('%s:%d: tmux subcommand %s not in §8.1.2' % (name, item.lineno, head))
            if isinstance(item, ast.Call) and callee(item) == 'call' and item.args and isinstance(item.args[0], ast.Constant):
                tool = item.args[0].value
                if isinstance(tool, str) and tool.startswith('devbox_') and tool not in mcp_allowed:
                    findings.append('%s:%d: MCP tool %s not in §8.1.3' % (name, item.lineno, tool))
    return findings


def scan_wsl_modules(sources):
    """wsl_daemon, agents_live and daemon_rows import nothing native and name no native script."""
    findings = []
    for name in PURE_WSL:
        tree = ast.parse(sources[name])
        for item in ast.walk(tree):
            if isinstance(item, ast.Import):
                findings += ['%s:%d: imports %s' % (name, item.lineno, a.name) for a in item.names if a.name in NATIVE_MODULES]
            elif isinstance(item, ast.ImportFrom):
                if item.module in NATIVE_MODULES:
                    findings.append('%s:%d: imports from %s' % (name, item.lineno, item.module))
                findings += ['%s:%d: imports %s' % (name, item.lineno, a.name) for a in item.names if a.name in NATIVE_NAMES]
            elif isinstance(item, ast.Name) and item.id in NATIVE_NAMES:
                findings.append('%s:%d: names %s' % (name, item.lineno, item.id))
            elif isinstance(item, ast.Attribute) and item.attr in NATIVE_NAMES:
                findings.append('%s:%d: names .%s' % (name, item.lineno, item.attr))
        findings += ['%s:%d: names a native program or script' % (name, line) for line, value in constants(tree) if NATIVE_TEXT.search(value)]
    return findings


def _references(node):
    """('self', attr) / ('attr', attr) / ('name', id) under node, with `if native_flow:` bodies pruned."""
    stack = [node]
    while stack:
        item = stack.pop()
        if isinstance(item, ast.If) and isinstance(item.test, ast.Name) and item.test.id == 'native_flow':
            stack.extend(item.orelse)
            continue
        if isinstance(item, ast.IfExp) and isinstance(item.test, ast.Name) and item.test.id == 'native_flow':
            stack.append(item.orelse)
            continue
        if isinstance(item, ast.Attribute):
            # self.X in Session, and self.s.X in the driver's ops classes (s is the Session)
            on_session = (isinstance(item.value, ast.Name) and item.value.id == 'self') or (
                isinstance(item.value, ast.Attribute) and item.value.attr == 's' and isinstance(item.value.value, ast.Name) and item.value.value.id == 'self')
            yield ('self' if on_session else 'attr'), item.attr
        elif isinstance(item, ast.Name):
            yield 'name', item.id
        stack.extend(ast.iter_child_nodes(item))


def scan_driver_wsl_paths(text, roots=WSL_ROOTS):
    """The driver's WSL-only paths call neither native() nor interop_probe() (changes §13)."""
    tree = ast.parse(text)
    classes = {n.name: n for n in tree.body if isinstance(n, ast.ClassDef)}
    functions = {n.name: n for n in tree.body if isinstance(n, ast.FunctionDef)}
    methods = {n.name: n for n in classes['Session'].body if isinstance(n, ast.FunctionDef)}
    seen, queue, findings = set(), [('self', r) for r in roots], []
    while queue:
        kind, name = queue.pop()
        if (kind, name) in seen:
            continue
        seen.add((kind, name))
        if name in NATIVE_NAMES:
            findings.append('driver.py: a WSL path reaches %s' % name)
            continue
        bodies = []
        if kind == 'self' and name in methods:
            bodies.append(methods[name])
        if kind == 'name' and name in classes and name != 'Session':
            bodies.append(classes[name])
        if kind == 'name' and name in functions:
            bodies.append(functions[name])
        for body in bodies:
            queue.extend(ref for ref in _references(body) if ref not in seen)
    return findings, sorted(n for k, n in seen if k == 'self' and n in methods)


def check_fixture():
    rows = {}
    for mode, root in (('windows', 'C:\\dev\\devbox-s7-accept-test'), ('wsl', None)):
        for upgrade in (False, True):
            plan = fixture_plan('a' * 40, 'owned-123456abcdef', mode, root, home=Path('/owned-home'), upgrade=upgrade)
            validate_plan(plan)
            if mode == 'windows' and tomllib.loads(plan['configLocalText']) != {'windows': {'root': root}}:
                raise ValueError('config.local.toml fixture invalid')
            if mode == 'wsl' and (plan['configLocalText'] is not None or plan['windowsRoot'] is not None):
                raise ValueError('The WSL fixture must have no config.local.toml')
            rows['%s%s' % (mode, '-upgrade' if upgrade else '')] = sorted(plan['projectPaths'])
    if fixture_root('owned-123456abcdef').exists():
        raise ValueError('fixture_plan wrote to disk')
    return rows


def powershell_parse(files):
    for file in files:
        win = run_checked(['wslpath', '-w', str(file)], timeout=10, text=True).stdout.strip().replace("'", "''")
        command = ("$errors=$null;$tokens=$null;[System.Management.Automation.Language.Parser]::ParseFile('" + win +
                   "',[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){$errors|Out-String|Write-Error;exit 1}")
        result = subprocess.run([PS, '-NoProfile', '-NonInteractive', '-Command', command], capture_output=True, text=True, encoding='utf-8',
                                errors='replace', timeout=60)
        if result.returncode:
            raise ValueError(file.name + ': ' + redact_text(result.stderr, 4000))


def load_wsl_owned():
    spec = importlib.util.spec_from_file_location('wsl_owned_validate', BASE / 'wsl-owned.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def manifest_files(base=BASE):
    return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(Path(base).iterdir())
            if p.is_file() and not p.is_symlink() and not MANIFEST_EXCLUDED.fullmatch(p.name)}


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--worktree', required=True)
    parser.add_argument('--source-sha')
    parser.add_argument('--powershell-parse', action='store_true')
    parser.add_argument('--write-manifest', action='store_true')
    args = parser.parse_args(argv)
    for file in sorted(BASE.glob('*.py')):
        ast.parse(file.read_text(encoding='utf-8'), filename=str(file))
    sources = helpers()
    wsl_owned = load_wsl_owned()
    import agents_live
    import mcp_stdio
    findings = (scan_forbidden(sources) + scan_gh(sources) + scan_tool_paths(sources)
                + scan_allowlists(sources, wsl_owned.READ_ONLY | wsl_owned.OWNED_MUTATIONS) + scan_wsl_modules(sources)
                + scan_ps1_encoding() + scan_ps1_launch(sources))
    driver_findings, wsl_methods = scan_driver_wsl_paths(sources['driver.py'])
    findings += driver_findings
    if set(agents_live.TMUX_READ) | set(agents_live.TMUX_MUTATE) != TMUX_SUBCOMMANDS or agents_live.TMUX != ('/usr/bin/tmux', '-L', 'devbox-accept'):
        findings.append('agents_live: tmux binary/socket or subcommand lists differ from §8.1.2')
    if set(mcp_stdio.TOOLS) != MCP_TOOLS:
        findings.append('mcp_stdio: TOOLS differ from §8.1.3')
    if findings:
        raise ValueError('Validation findings: ' + json.dumps(findings, ensure_ascii=False))
    root = verification_root()
    lock = root / 'windows-acceptance.lock'
    config = source_config()
    report = {'status': 'PASS', 'scope': 'static/pure preparation only (no install/launch/WSL units)',
              'PowerShellParse': 'NOT_RUN: needs interop; run with --powershell-parse in a Windows flow',
              'lockFile': 'present' if lock.is_file() else 'absent (created by verify-resources.py --lock windows-acceptance)',
              'forbidden': 'none', 'toolConfigPaths': 'trust_probe only', 'allowlists': 'consistent', 'wslPurity': 'PASS',
              'ps1Encoding': 'ASCII or UTF-8 BOM', 'ps1Launch': 'no Start-Process with redirection',
              'driverWslMethods': wsl_methods, 'scratch': 'none', 'fixture': check_fixture(), 'helperSha256': helper_hashes(BASE),
              'sourceBinding': 'UNBOUND' if not BINDING_FILE.exists() else {side: (config.get(side) or {}).get('sha') for side in ('candidate', 'baseline')}}
    if args.powershell_parse:
        powershell_parse(sorted(BASE.glob('*.ps1')))
        report['PowerShellParse'] = 'PASS'
    if args.source_sha:
        _, binding = probe_script(args.worktree, args.source_sha)
        report['probeBinding'] = binding
        if BINDING_FILE.exists():
            baseline = (config.get('baseline') or {}).get('sha')
            report['sourceVerified'] = verify_source(args.worktree, args.source_sha, baseline)['candidate']['sha']
        else:
            report['sourceVerified'] = 'NOT_BOUND'
    if args.write_manifest:
        dump(BASE / 'helper-manifest.json', {'schema': 2, 'status': 'PREPARED_NOT_REVIEWED', 'files': manifest_files(BASE),
                                             'runtimeHelperSha256': helper_hashes(BASE),
                                             'excluded': 'README.md, test_*, review-approval*, runs/, helper-manifest.json',
                                             'sourceBinding': report['sourceBinding'],
                                             'note': 'runtimeHelperSha256 is what review-approval.json helperSha256 must equal'})
        report['manifest'] = str(BASE / 'helper-manifest.json')
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0


if __name__ == '__main__':
    sys.exit(main())
