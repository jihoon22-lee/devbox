import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('dogfood', ROOT / 'scripts/dogfood.py')
DOGFOOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DOGFOOD)
SHA = 'a' * 40

class Dogfood(unittest.TestCase):
    def test_provenance_channel_and_download_isolation(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            existing = directory / 'devbox-dogfood'
            existing.mkdir()
            (existing / 'keep').write_text('user')
            receipt = dict(source_sha=SHA, run_id='42', repository=DOGFOOD.REPO, workflow='.github/workflows/windows.yml', version='1.0.0-dev', channel='prod', installers={'Devbox-setup.exe': hashlib.sha256(b'fixture').hexdigest()})
            detail = dict(id=42, head_sha=SHA, head_branch='fixture', path=receipt['workflow'], head_repository={'full_name': DOGFOOD.REPO}, repository={'full_name': DOGFOOD.REPO}, event='pull_request', status='completed', conclusion='success', html_url='fixture-run')
            calls, override = [], {}
            def command(*args):
                calls.append(args)
                if args[0] == 'wslpath' or args[0] == '/bin/true':
                    return str(directory)
                if args[:2] == ('gh', 'api'):
                    if '/git/ref/' in args[2]:
                        return json.dumps({'object': {'sha': override.get('remote_sha', SHA)}})
                    if args[2].endswith('/actions/runs'):
                        self.assertTrue({'--paginate', '--slurp', 'head_sha=' + SHA, 'branch=fixture'} <= set(args))
                        return json.dumps([{'workflow_runs': [dict(detail, path='other.yml'), dict(detail, id=1, event='push' if receipt['channel'] == 'accept' else 'schedule')]}, {'workflow_runs': [detail]}])
                    self.assertEqual(args[2], f'repos/{DOGFOOD.REPO}/actions/runs/42')
                    return json.dumps(dict(detail, **override.get('detail', {})))
                if args[:3] == ('gh', 'run', 'watch'):
                    if override.get('watch'):
                        raise subprocess.CalledProcessError(1, args)
                    return ''
                self.assertEqual(args[:3], ('gh', 'run', 'download'))
                self.assertEqual(args[args.index('-n') + 1], 'devbox-setup' + ('-accept' if receipt['channel'] == 'accept' else ''))
                destination = Path(args[args.index('-D') + 1])
                (destination / 'Devbox-setup.exe').write_bytes(override.get('bytes', b'fixture'))
                (destination / 'receipt.json').write_text(json.dumps(dict(receipt, **override.get('receipt', {}))))
            with patch.object(DOGFOOD, 'command', side_effect=command), patch.object(DOGFOOD.shutil, 'which', return_value='/bin/true'):
                for arguments in [['--channel', 'dev'], ['--channel', 'other'], ['fixture', 'extra']]:
                    with patch.object(DOGFOOD.sys, 'argv', ['dogfood.py', *arguments]), self.assertRaises(SystemExit):
                        DOGFOOD.main()
                for channel in ['prod', 'accept']:
                    receipt['channel'] = channel
                    with patch.object(DOGFOOD.sys, 'argv', ['dogfood.py', 'fixture'] + (['--channel', channel] if channel == 'accept' else [])):
                        DOGFOOD.main()
                        for override in [dict(remote_sha='invalid'), dict(watch=True), dict(detail={'event': 'push' if channel == 'accept' else 'schedule'})] + [dict(detail={key: value}) for key, value in [('head_sha', 'b' * 40), ('head_branch', 'other'), ('path', 'other.yml'), ('head_repository', {'full_name': 'other'}), ('repository', {'full_name': 'other'}), ('event', 'schedule'), ('status', 'queued'), ('conclusion', 'failure')]]:
                            calls.clear()
                            with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                                DOGFOOD.main()
                            self.assertFalse(any(call[:3] == ('gh', 'run', 'download') or call[0] == '/bin/true' for call in calls))
                        for override in [dict(bytes=b'tampered')] + [dict(receipt={key: value}) for key, value in [('channel', 'accept' if channel == 'prod' else 'prod'), ('source_sha', 'b' * 40), ('run_id', '43'), ('repository', 'other'), ('workflow', 'other.yml')]]:
                            with self.assertRaises(ValueError):
                                DOGFOOD.main()
                        override = {}
            self.assertEqual(len(list(directory.glob('devbox-dogfood-a*'))), 14)
            self.assertEqual((existing / 'keep').read_text(), 'user')
