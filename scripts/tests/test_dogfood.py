import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('dogfood', ROOT / 'scripts/dogfood.py')
DOGFOOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DOGFOOD)
SHA = 'a' * 40


class Dogfood(unittest.TestCase):
    def test_receipt_rejects_mismatch_and_tampering(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'Devbox-setup.exe').write_bytes(b'fixture')
            receipt = dict(source_sha=SHA, run_id='42', repository=DOGFOOD.REPO, workflow='.github/workflows/windows.yml', version='1.0.0-dev', installers={'Devbox-setup.exe': hashlib.sha256(b'fixture').hexdigest()})
            (directory / 'receipt.json').write_text(json.dumps(receipt))
            DOGFOOD.verify(directory, SHA, 42)
            with self.assertRaises(ValueError):
                DOGFOOD.verify(directory, 'b' * 40, 42)
            (directory / 'Devbox-setup.exe').write_bytes(b'tampered')
            with self.assertRaises(ValueError):
                DOGFOOD.verify(directory, SHA, 42)

    def test_first_pr_without_default_workflow_downloads_exclusively_and_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            downloads = directory / 'redirected Downloads'
            downloads.mkdir()
            existing = downloads / 'devbox-dogfood'
            existing.mkdir()
            (existing / 'keep').write_text('user')
            fake = directory / 'bin'
            fake.mkdir()
            stub = '''#!/usr/bin/env python3
import hashlib,json,os,pathlib,sys
args=sys.argv[1:]; name=pathlib.Path(sys.argv[0]).name
with open(os.environ['CALLS'],'a') as log: log.write(name+' '+repr(args)+'\\n')
if name=='powershell.exe': print(os.environ['DOWNLOADS'])
elif name=='wslpath': print(args[-1])
elif args[:1]==['api']:
 if '/contents/' in args[1] or '/actions/workflows/' in args[1]: sys.exit(44) # Workflow not on default branch.
 elif args[1].endswith('/actions/runs'):
  assert '--paginate' in args and '--slurp' in args and 'head_sha='+'a'*40 in args and 'branch=fixture' in args
  run=dict(id=42,head_sha='a'*40,head_branch='fixture',path='.github/workflows/windows.yml',event='pull_request')
  print(json.dumps([{'workflow_runs':[dict(run,id=1,path='.github/workflows/ci.yml')]},{'workflow_runs':[run]}]))
 elif '/git/ref/' in args[1]: print(json.dumps({'object':{'sha':'a'*40}}))
 else: print(json.dumps(dict(head_sha=os.environ.get('RUN_SHA','a'*40), head_branch='fixture', path='.github/workflows/windows.yml', head_repository={'full_name':'jihoon22-lee/devbox'}, repository={'full_name':'jihoon22-lee/devbox'}, event='pull_request',status='completed',conclusion='success',html_url='fixture-run')))
elif args[:2]==['run','list']: sys.exit(44) # Filename resolution requires default branch workflow.
elif args[:2]==['run','watch']: sys.exit(int(os.environ.get('WATCH_EXIT','0')))
elif args[:2]==['run','download']:
 dest=pathlib.Path(args[args.index('-D')+1]); (dest/'Devbox-setup.exe').write_bytes(b'fixture')
 (dest/'receipt.json').write_text(json.dumps(dict(source_sha='a'*40,run_id='42',repository='jihoon22-lee/devbox',workflow='.github/workflows/windows.yml',version='1.0.0-dev',installers={'Devbox-setup.exe':hashlib.sha256(b'fixture').hexdigest()})))
else: sys.exit(99)
'''
            for name in ['gh', 'powershell.exe', 'wslpath']:
                path = fake / name
                path.write_text(stub)
                path.chmod(0o700)
            log = directory / 'calls'
            env = dict(os.environ, PATH=str(fake) + os.pathsep + os.environ['PATH'], DOWNLOADS=str(downloads), CALLS=str(log))
            for _ in range(2):
                result = subprocess.run(['bash', str(ROOT / 'scripts/dogfood.sh'), 'fixture'], env=env, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(list(downloads.glob('devbox-dogfood-a*'))), 2)
            self.assertEqual((existing / 'keep').read_text(), 'user')
            self.assertNotIn("gh ['workflow', 'run'", log.read_text())
            self.assertNotIn("--workflow", log.read_text())
            self.assertIn("/actions/runs'", log.read_text())
            log.write_text('')
            result = subprocess.run(['bash', str(ROOT / 'scripts/dogfood.sh'), 'fixture'], env=dict(env, WATCH_EXIT='1'), capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('download', log.read_text())
            log.write_text('')
            result = subprocess.run(['bash', str(ROOT / 'scripts/dogfood.sh'), 'fixture'], env=dict(env, RUN_SHA='b' * 40), capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('download', log.read_text())
            self.assertNotIn('powershell.exe', log.read_text())
