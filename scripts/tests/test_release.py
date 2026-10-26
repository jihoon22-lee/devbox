import contextlib
import copy
import hashlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SHA, TAG = 'a' * 40, 'v1.2.3'
NOTES = 'Notes "quoted" `literal` $(touch escaped)\nsecond line\n\n'
ASSET = dict(id=9, name='devbox-1.2.3-setup.exe', size=7, state='uploaded', digest='sha256:' + hashlib.sha256(b'fixture').hexdigest())

class Release(unittest.TestCase):
    def scenario(self, change=None, existing=None, failure=False, writes=None, preflight=False):
        with tempfile.TemporaryDirectory() as temporary, contextlib.chdir(temporary):
            root = Path(temporary)
            for name, text in [('Cargo.toml', '[workspace.package]\nversion="1.2.3"'), ('app/src-tauri/Cargo.toml', '[package]\nversion="1.2.3"'), ('app/package.json', '{"version":"1.2.3"}'), ('app/src-tauri/tauri.conf.json', '{"version":"1.2.3"}')]:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            directory = root / 'artifact'
            directory.mkdir()
            (directory / 'Devbox-setup.exe').write_bytes(b'fixture')
            receipt = dict(source_sha=SHA, run_id='42', repository='jihoon22-lee/devbox', workflow='.github/workflows/windows.yml', channel='prod', version='1.2.3', installers={'Devbox-setup.exe': ASSET['digest'][7:]})
            release = dict(id=7, tag_name=TAG, name=TAG, body=NOTES, target_commitish=SHA, prerelease=False, draft=existing != 'public') if existing else None
            state = dict(release=release, assets=[copy.deepcopy(ASSET)] if existing else [], notes=NOTES)
            environment = dict(GITHUB_EVENT_NAME='push', GITHUB_REF='refs/tags/' + TAG, SOURCE_SHA=SHA, GITHUB_RUN_ID='42', GITHUB_REPOSITORY=receipt['repository'], GH_TOKEN='fixture-only', GH_CONFIG_DIR=str(root / 'empty-gh'))
            calls = []
            if change:
                change(state, receipt, environment, directory)
            (directory / 'receipt.json').write_text(json.dumps(receipt))
            def fake(args, **kwargs):
                self.assertNotIn('shell', kwargs)
                if args[0] == 'git':
                    if args[1:3] == ('cat-file', 'tag'):
                        if state.get('lightweight'):
                            raise subprocess.CalledProcessError(1, args)
                        return ('object ' + SHA + '\ntype commit\ntag ' + TAG + '\n\n' + state['notes']).encode()
                    if args[1] == 'ls-remote':
                        return 'b' * 40 + '\trefs/tags/' + TAG + '\n' + state.get('remote', SHA) + '\trefs/tags/' + TAG + '^{}'
                    self.assertEqual(args[:2], ('git', 'rev-parse'))
                    return 'b' * 40 if args[-1] == 'refs/tags/' + TAG else SHA
                self.assertEqual(args[:2], ('gh', 'api'))
                endpoint = args[2]
                method = args[args.index('--method') + 1]
                calls.append(method)
                if state.get('lookup_error') and method == 'GET':
                    raise subprocess.CalledProcessError(1, args, output='HTTP ' + str(state['lookup_error']))
                if endpoint.endswith('/releases') and method == 'GET':
                    self.assertTrue({'--paginate', '--slurp'} <= set(args))
                    return json.dumps([[], [state['release']] * (2 if state.get('duplicate') else 1) if state['release'] else []])
                if method == 'POST' and endpoint.endswith('/releases'):
                    payload = json.loads(Path(args[args.index('--input') + 1]).read_text())
                    self.assertEqual(payload['body'], NOTES)
                    self.assertTrue(payload['draft'])
                    state['release'] = dict(payload, id=7)
                elif method == 'POST':
                    self.assertEqual(Path(args[args.index('--input') + 1]).read_bytes(), b'fixture')
                    self.assertEqual(endpoint, 'https://uploads.github.com/repos/jihoon22-lee/devbox/releases/7/assets?name=devbox-1.2.3-setup.exe')
                    state['assets'] = [dict(ASSET, **state.get('uploaded', {}))]
                    return json.dumps(state['assets'][0])
                elif endpoint.endswith('/assets'):
                    return json.dumps([state['assets']])
                elif endpoint.endswith('/assets/9'):
                    return json.dumps(dict(state['assets'][0], **state.get('fresh', {})))
                else:
                    self.assertEqual(endpoint, 'repos/jihoon22-lee/devbox/releases/7')
                    self.assertIn(method, ['GET', 'PATCH'])
                    if method == 'PATCH':
                        self.assertEqual(json.loads(Path(args[args.index('--input') + 1]).read_text()), {'draft': False})
                        self.assertEqual(state['assets'], [ASSET])
                        self.assertGreaterEqual(calls.count('GET'), 4)
                        state['release']['draft'] = False
                return json.dumps(state['release'])
            with patch.dict(os.environ, environment, clear=True), patch.object(sys, 'path', [str(ROOT / 'scripts'), *sys.path]), patch.object(sys, 'argv', ['release.py'] + ([] if preflight else [str(directory)])), patch.object(subprocess, 'check_output', side_effect=fake), patch.object(subprocess, 'Popen', side_effect=AssertionError('Unmocked process')):
                if failure:
                    with self.assertRaises((ValueError, subprocess.CalledProcessError, KeyError)):
                        runpy.run_path(str(ROOT / 'scripts/release.py'), run_name='__main__')
                else:
                    runpy.run_path(str(ROOT / 'scripts/release.py'), run_name='__main__')
            self.assertEqual([call for call in calls if call != 'GET'], writes or [])
            self.assertFalse((root / 'escaped').exists())

    def test_success_and_retries(self):
        for options in [dict(writes=['POST', 'POST', 'PATCH']), dict(existing='draft', writes=['PATCH']), dict(existing='public'), dict(preflight=True)]:
            with self.subTest(options=options):
                self.scenario(**options)

    def test_event_gate(self):
        for key, value in [('GITHUB_EVENT_NAME', 'pull_request'), ('GITHUB_EVENT_NAME', 'workflow_dispatch'), ('GITHUB_REF', 'refs/heads/main')]:
            self.scenario(lambda s, r, e, d: e.update({key: value}))

    def test_preflight_rejections(self):
        for tag in ['v01.2.3', 'v1.2.3-dev', 'v1.2.3+meta', 'v1.2', 'v1.2.4']:
            self.scenario(lambda s, r, e, d: e.update(GITHUB_REF='refs/tags/' + tag), failure=True)
        for state in [dict(lightweight=True), dict(remote='c' * 40), dict(duplicate=True)] + [dict(lookup_error=status) for status in [401, 403, 404, 429, 500]]:
            self.scenario(lambda s, r, e, d: s.update(state), existing='draft', failure=True)
        self.scenario(lambda s, r, e, d: e.update(SOURCE_SHA='c' * 40), failure=True)
        for name in ['Cargo.toml', 'app/package.json', 'app/src-tauri/Cargo.toml', 'app/src-tauri/tauri.conf.json']:
            self.scenario(lambda s, r, e, d: Path(name).write_text(Path(name).read_text().replace('1.2.3', '1.2.4')), failure=True)

    def test_receipt_rejections(self):
        for key, value in [('source_sha', 'c' * 40), ('run_id', '43'), ('repository', 'other'), ('workflow', 'other.yml'), ('channel', 'accept'), ('version', '1.2.4'), ('installers', {}), ('installers', {'Devbox-setup.exe': '0' * 64})]:
            self.scenario(lambda s, r, e, d: r.update({key: value}), failure=True)
        self.scenario(lambda s, r, e, d: (d / 'second.exe').write_bytes(b'fixture'), failure=True)
        self.scenario(lambda s, r, e, d: r['installers'].update({'second.exe': ASSET['digest'][7:]}) or (d / 'second.exe').write_bytes(b'fixture'), failure=True)
        self.scenario(lambda s, r, e, d: (d / 'Devbox-setup.exe').unlink(), failure=True)

    def test_asset_rejections(self):
        for change in [dict(digest='sha256:' + '0' * 64), dict(digest=None), dict(state='starter'), dict(size=0), dict(name='other.exe')]:
            for existing in ['draft', 'public']:
                self.scenario(lambda s, r, e, d: s['assets'][0].update(change), existing=existing, failure=True)
            self.scenario(lambda s, r, e, d: s.update(uploaded=change), failure=True, writes=['POST', 'POST'])
        self.scenario(lambda s, r, e, d: s['assets'].append(dict(ASSET)), existing='public', failure=True)
        self.scenario(lambda s, r, e, d: s.update(fresh=dict(digest=None)), failure=True, writes=['POST', 'POST'])
        self.scenario(lambda s, r, e, d: s['assets'].clear(), existing='draft', writes=['POST', 'PATCH'])
        self.scenario(lambda s, r, e, d: s['assets'].clear(), existing='public', failure=True)
        for change in [dict(body='foreign'), dict(prerelease=True), dict(target_commitish='c' * 40)]:
            self.scenario(lambda s, r, e, d: s['release'].update(change), existing='draft', failure=True)

    def test_workflow_gates_and_musl_failure(self):
        workflow = (ROOT / '.github/workflows/windows.yml').read_text()
        release = workflow.split('  release:\n')[1]
        for required in ["needs: [changes, linux-bin, windows]", "if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')", 'fetch-depth: 0', 'name: devbox-setup, path: installer', 'GH_TOKEN: ${{ github.token }}']:
            self.assertIn(required, release)
        self.assertNotIn('always()', release)
        self.assertEqual(workflow.count('contents: write'), 1)
        self.assertIn('fetch-depth: 0', workflow.split('  linux-bin:')[0])
        command = next(line.split('- run: ')[1] for line in workflow.splitlines() if 'cargo test --locked -p devbox-cli' in line)
        self.assertEqual(command, 'python3 scripts/verify-resources.py -- cargo test --locked -p devbox-cli --lib --release --target x86_64-unknown-linux-musl')
        self.assertLess(workflow.index(command), workflow.index('bash scripts/build-linux.sh release'))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fake = root / 'python3'
            fake.write_text('#!/bin/sh\nexit 42\n')
            fake.chmod(0o700)
            result = subprocess.run(['/bin/bash', '-eo', 'pipefail', '-c', command + '\ntouch built'], cwd=root, env={'PATH':str(root)}, capture_output=True)
            self.assertEqual(result.returncode, 42)
            self.assertFalse((root / 'built').exists())
