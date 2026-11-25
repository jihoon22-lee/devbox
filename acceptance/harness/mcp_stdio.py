"""`devbox --instance accept mcp` over stdio against the owned accept instance (changes §8.1.3).

Only two tool names and the fixed send texts are accepted. The binary is the pinned installed accept
link, or the kept rc.1 binary whose sha256 must equal the baseline receipt; it runs from the verified
descriptor, so a replaced path never redirects execution. Nothing here is imported by a Windows helper.
"""
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import threading
import time

from common import redact_json, require_lock
from installed_binary import digest_fd, installed_binary
import agents_live

TOOLS = ('devbox_agents', 'devbox_agent_send')
DEADLINE_SECONDS = 10.0
TERM_GRACE_SECONDS = 2.0
EVIDENCE_LIMIT = 20000


def argv_for(binary):
    return [str(binary), '--instance', 'accept', 'mcp']


def environment(home):
    return {'HOME': str(home), 'PATH': '/usr/bin:/bin', 'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}'}


def open_baseline(home, baseline_sha256):
    """The kept rc.1 binary `bin/devbox-<baseline sha256>`: a private regular file with exactly that hash."""
    directory = Path(home) / '.local/share/devbox-accept/bin'
    name = 'devbox-' + baseline_sha256
    fd = os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        st = os.fstat(fd)
        if not stat_regular(st) or st.st_uid != os.getuid() or digest_fd(fd) != baseline_sha256:
            raise ValueError('rc.1 MCP binary must hash to the baseline receipt')
        return fd, str(directory / name)
    except Exception:
        os.close(fd)
        raise


def stat_regular(st):
    return stat.S_ISREG(st.st_mode) and bool(st.st_mode & stat.S_IXUSR)


class McpClient:
    """One owned MCP stdio process; every request has a 10 s deadline; close() always ends it."""

    def __init__(self, load, home, record_process, baseline_sha256=None, popen=subprocess.Popen, clock=time):
        # load() returns the current on-disk manifest: agents created later are authorized too.
        self.load, self.home, self.record_process, self.baseline = load, Path(home), record_process, baseline_sha256
        self.popen, self.clock = popen, clock
        self.child = None
        self.pairs = []
        self.size = 0
        self.lines = []
        self.cv = threading.Condition()
        self.next_id = 1

    def start(self):
        require_lock()
        m = self.load()
        if self.baseline is None:
            with installed_binary(Path(m['acceptBinary']), m['linuxArtifacts'][m['phase']]['sha256'], home=self.home) as (fd, target):
                self._spawn(fd, target)
        else:
            fd, target = open_baseline(self.home, self.baseline)
            try:
                self._spawn(fd, target)
            finally:
                os.close(fd)
        self.request('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 's7-acceptance', 'version': '1'}})
        self.notify('notifications/initialized')
        return self

    def _spawn(self, fd, target):
        self.target = target
        self.child = self.popen(argv_for(target), executable='/proc/self/fd/%d' % fd, pass_fds=(fd,), stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=environment(self.home), close_fds=True)
        # The identity cleanup needs to end it even if this process dies (changes §11 step 1).
        self.record_process({'pid': self.child.pid, 'start': (Path('/proc') / str(self.child.pid) / 'stat').read_text().rsplit(')', 1)[1].split()[19],
                             'binary': target})
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self):
        for raw in iter(self.child.stdout.readline, b''):
            with self.cv:
                if len(self.lines) < 400:
                    self.lines.append(raw[:200000])
                self.cv.notify_all()
        with self.cv:
            self.cv.notify_all()

    def _write(self, message):
        body = json.dumps(message, ensure_ascii=False)
        self.child.stdin.write(body.encode() + b'\n')
        self.child.stdin.flush()
        return body

    def _keep(self, request, response):
        """M7: each side pretty-printed and redacted line by line (common.redact_json), within the 20 kB bound."""
        row = {'request': redact_json(request), 'response': redact_json(response)}
        size = len(json.dumps(row, ensure_ascii=False))
        if self.size + size <= EVIDENCE_LIMIT:
            self.pairs.append(row)
            self.size += size
        else:
            self.pairs.append({'request': redact_json(request, 200), 'response': '(omitted: 20 kB evidence bound)'})

    def notify(self, method):
        self._keep(self._write({'jsonrpc': '2.0', 'method': method}), None)

    def request(self, method, params):
        if method not in ('initialize', 'tools/list', 'tools/call'):
            raise ValueError('MCP method not allowed: ' + method)
        ident = self.next_id
        self.next_id += 1
        sent = self._write({'jsonrpc': '2.0', 'id': ident, 'method': method, **({'params': params} if params is not None else {})})
        deadline = self.clock.monotonic() + DEADLINE_SECONDS
        with self.cv:
            while True:
                for raw in list(self.lines):
                    try:
                        value = json.loads(raw)
                    except ValueError:
                        continue
                    if isinstance(value, dict) and value.get('id') == ident:
                        self.lines.remove(raw)
                        self._keep(sent, raw.decode('utf-8', 'replace'))
                        return value
                remaining = deadline - self.clock.monotonic()
                if remaining <= 0 or (self.child.poll() is not None and not self.lines):
                    self._keep(sent, '(no reply within 10 s)')
                    raise TimeoutError('MCP reply deadline: ' + method)
                self.cv.wait(min(remaining, 0.1))

    def tools_list(self):
        return self.request('tools/list', None)

    def call(self, name, arguments):
        authorize_call(self.load(), name, arguments)
        return self.request('tools/call', {'name': name, 'arguments': arguments})

    def send(self, agent_id, text):
        """devbox_agent_send; returns ('accepted'|'refused', the tool's text)."""
        reply = self.call('devbox_agent_send', {'id': agent_id, 'text': text})
        result = reply.get('result') or {}
        content = ''.join(part.get('text', '') for part in result.get('content') or [] if isinstance(part, dict))
        return ('refused' if result.get('isError') or 'error' in reply else 'accepted'), content

    def close(self):
        if self.child is None or self.child.poll() is not None:
            return {'state': 'not-running'}
        try:
            self.child.stdin.close()
        except OSError:
            pass
        self.child.send_signal(signal.SIGTERM)
        try:
            self.child.wait(timeout=TERM_GRACE_SECONDS)
            return {'state': 'terminated', 'exit': self.child.returncode}
        except subprocess.TimeoutExpired:
            self.child.kill()
            self.child.wait(timeout=5)
            return {'state': 'killed', 'exit': self.child.returncode}


def authorize_call(m, name, arguments):
    """Only devbox_agents and devbox_agent_send, with recorded agents and the fixed texts (changes §8.1.3)."""
    if name not in TOOLS:
        raise ValueError('MCP tool not allowed: ' + str(name))
    if name == 'devbox_agents':
        if arguments != {}:
            raise ValueError('devbox_agents takes no arguments here')
        return
    if not isinstance(arguments, dict) or set(arguments) != {'id', 'text'}:
        raise ValueError('devbox_agent_send takes exactly id and text (crates/cli/src/mcp/tools.rs)')
    if arguments['id'] not in m.get('agentIds', []) or arguments['text'] not in agents_live.send_texts(m['nonce']):
        raise ValueError('devbox_agent_send limited to recorded agents and the fixed texts')
