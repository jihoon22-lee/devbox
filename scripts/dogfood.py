#!/usr/bin/env python3
"""Download a verified Windows CI installer into a fresh Windows Downloads directory."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time
from urllib.parse import quote

REPO = 'jihoon22-lee/devbox'

def command(*args):
    return subprocess.check_output(args, text=True).strip()

def gh_json(*args):
    return json.loads(command('gh', *args))

def verify(directory, sha, run, channel='prod'):
    receipt = json.loads((directory / 'receipt.json').read_text(encoding='utf-8-sig'))
    if (receipt['source_sha'], str(receipt['run_id']), receipt['repository'], receipt['workflow'], receipt.get('channel')) != (sha, str(run), REPO, '.github/workflows/windows.yml', channel):
        raise ValueError('Artifact provenance mismatch')
    files = receipt['installers']
    if not files or not receipt['version']:
        raise ValueError('Empty installer receipt')
    if set(files) != {path.name for path in directory.glob('*.exe')}:
        raise ValueError('Installer list mismatch')
    for name, expected in files.items():
        path = directory / name
        if Path(name).name != name or '/' in name or '\\' in name or not name.endswith('.exe') or path.is_symlink():
            raise ValueError('Invalid installer filename')
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected.lower():
            raise ValueError('Installer SHA256 mismatch')
    return receipt

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('branch', nargs='?')
    parser.add_argument('--channel', choices=('prod', 'accept'), default='prod')
    args = parser.parse_args()
    branch = args.branch or command('git', 'branch', '--show-current')
    if not branch:
        raise ValueError('Expected a pushed branch')
    sha = gh_json('api', f'repos/{REPO}/git/ref/heads/{quote(branch, safe="/")}')['object']['sha']
    if len(sha) != 40 or any(c not in '0123456789abcdef' for c in sha):
        raise ValueError('Invalid remote SHA')

    def find_run():
        # Repository runs work before the workflow file exists on the default branch.
        pages = gh_json('api', f'repos/{REPO}/actions/runs', '--method', 'GET', '--paginate', '--slurp',
                        '-f', f'head_sha={sha}', '-f', f'branch={branch}', '-f', 'per_page=100')
        return next((run['id'] for page in pages for run in page['workflow_runs']
                     if run['path'] == '.github/workflows/windows.yml' and run['head_sha'] == sha
                     and run['head_branch'] == branch and run['event'] in ('pull_request', 'workflow_dispatch', 'push') and (args.channel == 'prod' or run['event'] != 'push')), None)

    run = find_run()
    if run is None:
        # A new workflow on a PR should already have a PR run. Dispatch is possible only after it exists on default branch.
        gh_json('api', f'repos/{REPO}/contents/.github/workflows/windows.yml')
        print('No matching run; dispatching windows.yml for remote SHA ' + sha, flush=True)
        command('gh', 'workflow', 'run', 'windows.yml', '--repo', REPO, '--ref', branch)
        for _ in range(36):
            time.sleep(5)
            run = find_run()
            if run is not None:
                break
        if run is None:
            raise ValueError('No matching Windows run after dispatch; check workflow availability on main')
    command('gh', 'run', 'watch', str(run), '--repo', REPO, '--exit-status')
    detail = gh_json('api', f'repos/{REPO}/actions/runs/{run}')
    if (detail['head_sha'] != sha or detail['head_branch'] != branch or detail['path'] != '.github/workflows/windows.yml'
            or detail['head_repository']['full_name'] != REPO or detail['repository']['full_name'] != REPO
            or detail['event'] not in ('pull_request', 'workflow_dispatch', 'push') or (args.channel == 'accept' and detail['event'] == 'push') or detail['status'] != 'completed' or detail['conclusion'] != 'success'):
        raise ValueError('Windows run provenance/status mismatch')
    # Shell namespace resolves redirected/localized Downloads; USERNAME is not a profile path.
    powershell = shutil.which('powershell.exe') or '/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe'
    if not Path(powershell).is_file() or not os.access(powershell, os.X_OK):
        raise ValueError('Windows PowerShell is unavailable')
    downloads = command(powershell, '-NoProfile', '-NonInteractive', '-Command',
                        "[Console]::OutputEncoding = New-Object Text.UTF8Encoding; (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path")
    directory = Path(command('wslpath', '-u', downloads))
    if not directory.is_dir():
        raise ValueError('Windows Downloads directory is unavailable')
    destination = Path(tempfile.mkdtemp(prefix=f'devbox-dogfood-{sha[:12]}-{run}-', dir=directory))
    (destination / '.devbox-dogfood-owner.json').write_text(json.dumps({'sha': sha, 'run': run, 'channel': args.channel}))
    command('gh', 'run', 'download', str(run), '--repo', REPO, '-n', 'devbox-setup' + ('-accept' if args.channel == 'accept' else ''), '-D', str(destination))
    receipt = verify(destination, sha, run, args.channel)
    print(f'Verified source: {sha}\nRun: {detail["html_url"]}\nChannel: {receipt["channel"]}\nVersion: {receipt["version"]}\nReceipt: {destination / "receipt.json"}')
    print('Windows installer directory: ' + command('wslpath', '-w', str(destination)))

if __name__ == '__main__':
    try:
        main()
    except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        sys.exit(f'Dogfood failed (workflow must be available on main for dispatch): {error}')
