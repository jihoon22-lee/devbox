#!/usr/bin/env python3
"""Own only development children; never install/start/stop a user service."""
import json
import os
from pathlib import Path
import re
import secrets
import select
import signal
import socket
import struct
import subprocess
import time
import threading
import urllib.request

def hello(path):
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(1)
        sock.connect(str(path))
        body = json.dumps(dict(type='hello', version=json.loads(Path('app/package.json').read_text())['version'], protocol=1, client='dev', instance='dev')).encode()
        sock.sendall(struct.pack('<IBI', len(body) + 5, 0, 0) + body)
        def read(size):
            result = b''
            while len(result) < size:
                chunk = sock.recv(size - len(result))
                if not chunk:
                    raise OSError('daemon closed before welcome')
                result += chunk
            return result
        size = struct.unpack('<I', read(4))[0]
        if not 5 <= size <= 4096:
            raise OSError('invalid welcome frame')
        frame = read(size)
        welcome = json.loads(frame[5:])
        if (frame[:5] != b'\0' * 5 or welcome.get('type') != 'welcome'
                or welcome.get('protocol') != 1
                or welcome.get('version') != json.loads(Path('app/package.json').read_text())['version']):
            raise OSError('daemon welcome required')

def runtime_directory(env):
    candidate = Path(env.get("XDG_RUNTIME_DIR") or "")
    return candidate if candidate.is_absolute() else Path(f"/run/user/{os.getuid()}")

def vite_announced(log, port):
    plain = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", log)
    return bool(re.search(rf"^[\t ]*(?:➜[\t ]*)?Local:[\t ]+http://(?:localhost|127\.0\.0\.1):{port}/[\t ]*\r?$", plain, re.MULTILINE))

def main():
    children = []
    def stop(_signum, _frame):
        raise KeyboardInterrupt
    # Handlers precede the first child; finally also covers partial startup failures.
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, stop)
    def start(command, env, **options):
        child = subprocess.Popen(command, env=env, start_new_session=True, **options)
        children.append(child)
        return child
    def ready(probe, seconds=20):
        deadline = time.monotonic() + seconds
        while True:
            if any(child.poll() is not None for child in children):
                raise RuntimeError('development child exited during startup')
            try:
                probe()
                return
            except (OSError, ValueError):
                if time.monotonic() >= deadline:
                    raise RuntimeError('development readiness timed out')
                time.sleep(0.1)
    try:
        meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--format-version', '1', '--no-deps']))
        binary = str(Path(meta['target_directory']) / 'debug/devbox')
        env = dict(os.environ, DEVBOX_INSTANCE='dev')
        runtime = runtime_directory(env)
        path = runtime / 'devbox-dev.sock'
        try:
            hello(path)
        except (OSError, ValueError):
            start([binary, 'daemon', '--foreground'], env)
            ready(lambda: hello(path))
        token = secrets.token_hex(24)
        gateway = start([binary, 'dev-gateway', '--port', '1451', '--token', token], env, stdout=subprocess.PIPE)
        def gateway_ready():
            # The gateway prints this only after its own successful bind.
            if not select.select([gateway.stdout], [], [], 0.1)[0]:
                raise OSError('gateway has not bound')
            line = gateway.stdout.readline(4096).decode()
            if line.strip() != f'dev-gateway ws://127.0.0.1:1451/ws?token={token}':
                raise RuntimeError('gateway did not announce its owned listener')
        ready(gateway_ready)
        vite = str(Path('app/node_modules/vite/bin/vite.js').resolve())
        frontend = start(['node', vite, '--host', '127.0.0.1', '--port', '1450', '--strictPort', 'app'],
              dict(env, VITE_DEVBOX_WS=f'ws://127.0.0.1:1451/ws?token={token}', VITE_DEVBOX_INSTANCE='dev'), stdout=subprocess.PIPE)
        bound = threading.Event()
        def relay():
            log = ""
            for raw in frontend.stdout:
                line = raw.decode(errors='replace')
                print(line, end='', flush=True)
                log = (log + line)[-16384:]
                if vite_announced(log, 1450):
                    bound.set()
        threading.Thread(target=relay, daemon=True).start()
        def frontend_ready():
            if not bound.is_set():
                raise OSError('Vite has not bound its own listener')
            with urllib.request.urlopen('http://127.0.0.1:1450', timeout=1) as response:
                if response.status != 200:
                    raise OSError('frontend not ready')
        ready(frontend_ready)
        print('Devbox: http://localhost:1450', flush=True)
        while all(child.poll() is None for child in children):
            time.sleep(0.25)
        raise RuntimeError('development child exited')
    finally:
        for child in reversed(children):
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        for child in reversed(children):
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()

if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
