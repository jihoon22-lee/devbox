"""Narrow provenance contract for unchanged candidate bytes and corrected fixtures.

The trusted main workflow creates this receipt after authenticated API lookups;
consumers bind it to that workflow execution and independently check asset bytes.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

WORKFLOW = '.github/workflows/windows-candidate-revalidation.yml'
REQUIRED_JOBS = (
    'Validate source and plan package shards',
    'Build required static Workspace component',
    'Build Windows products (shard 01)',
    'Build Windows products (shard 02)',
    'Build Windows products (shard 03)',
    'Assemble and verify unpublished candidate',
    'Packaged native acceptance (product-shells)',
    'Packaged native acceptance (api)',
    'Packaged native acceptance (knowledge)',
    'Packaged native acceptance (cross-product)',
    'Exact candidate WSL2 and Docker acceptance on disposable VM',
)
# Every other tracked blob AND mode must match, including build scripts/locks.
# These exact reviewed paths only affect evidence, never packaged product inputs.
ALLOWED_CHANGES = frozenset({
    '.github/scripts/windows-suite-ui-context.mjs',
    '.github/scripts/windows-suite-ui-close.test.mjs',
    '.github/scripts/windows-suite-delivery-native.mjs',
    '.github/scripts/windows-suite-user-flow.mjs',
    '.github/scripts/windows-suite-agent-user-flows.mjs',
    '.github/scripts/windows-suite-direct-layout.mjs',
    '.github/scripts/windows-suite-direct-layout.test.mjs',
    '.github/scripts/suite-user-flow-results.mjs',
    '.github/scripts/collect-user-flow-evidence.mjs',
    '.github/scripts/suite-user-flow-evidence.mjs',
    '.github/scripts/suite-user-flow-evidence.test.mjs',
    '.github/scripts/candidate_revalidation.py',
    '.github/scripts/prepare-candidate-revalidation.py',
    '.github/scripts/test-candidate-revalidation.py',
    '.github/scripts/build-candidate-metadata.py',
    '.github/scripts/test-build-candidate-metadata.py',
    '.github/scripts/candidate_user_flow.py',
    '.github/scripts/test-candidate-user-flow.py',
    '.github/scripts/verify-downloaded-release.py',
    '.github/scripts/test-verify-downloaded-release.py',
    '.github/scripts/test-windows-candidate-revalidation-config.py',
    '.github/scripts/revalidation-identity.mjs',
    '.github/scripts/revalidation-identity.test.mjs',
    '.github/scripts/windows-user-flow-install.ps1',
    '.github/scripts/windows-product-foundation.mjs',
    '.github/scripts/windows-api-user-flow-actions.mjs',
    '.github/scripts/windows-api-user-flow-adapter.test.mjs',
    '.github/scripts/windows-knowledge-activity.mjs',
    '.github/scripts/windows-knowledge-user-flows.test.mjs',
    '.github/scripts/windows-suite-integration.mjs',
    '.github/scripts/windows-suite-integration.test.mjs',
    '.github/scripts/windows-suite-delivery-user-flows.mjs',
    '.github/scripts/windows-suite-delivery-reopen.test.mjs',
    'AGENTS.md',
    'CONVENTIONS.md',
    '.github/workflows/windows-candidate-revalidation.yml',
    'docs/release-policy.md',
    'docs/release-evidence.md',
    'docs/verification.md',
    'docs/superpowers/plans/2026-10-03-product-readiness/08-traceability.md',
})

def require(condition, message):
    if not condition:
        raise ValueError(message)

def positive(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0

def digest(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value) is not None

def sha(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{40}', value) is not None

def compare_inputs(before, after):
    changes = []
    for name in sorted(before.keys() | after.keys()):
        if before.get(name) == after.get(name):
            continue
        require(name in ALLOWED_CHANGES, f'Packaged or unreviewed input changed: {name}')
        old, new = before.get(name), after.get(name)
        require(all(item is None or item[0] in ('100644', '100755') for item in (old, new)), 'Linked fixture inputs forbidden')
        changes.append(dict(path=name, before=list(old) if old else None, after=list(new) if new else None))
    return changes

def git_tree(commit):
    output = subprocess.check_output(['git', 'ls-tree', '-rz', '--full-tree', commit])
    tree = {}
    for item in output.split(b'\0'):
        if not item:
            continue
        header, name = item.split(b'\t', 1)
        mode, kind, oid = header.decode().split()
        tree[name.decode()] = (mode, oid)
    return tree

def verified_changes(source, fixture):
    require(sha(source) and sha(fixture), 'Invalid source or fixture commit')
    require(subprocess.run(['git', 'merge-base', '--is-ancestor', source, fixture], check=False).returncode == 0,
            'Build source must be an ancestor of the fixture')
    return compare_inputs(git_tree(source), git_tree(fixture))

def checked_jobs(jobs):
    selected = []
    require(isinstance(jobs, list), 'Missing authenticated job list')
    for name in REQUIRED_JOBS:
        matches = [job for job in jobs if job.get('name') == name]
        require(len(matches) == 1, f'Required reused job missing/duplicate: {name}')
        job = matches[0]
        require(positive(job.get('id')) and job.get('status') == 'completed' and job.get('conclusion') == 'success',
                f'Required reused job did not pass: {name}')
        selected.append({key: job[key] for key in ('id', 'name', 'status', 'conclusion')})
    return selected

def validate_original_run(run, jobs, repository, source, build_run):
    require(isinstance(run, dict) and run.get('id') == build_run
            and run.get('path') == '.github/workflows/windows-package-candidate.yml'
            and run.get('head_sha') == source and run.get('head_branch') == 'main'
            and run.get('head_repository', {}).get('full_name') == repository
            and run.get('event') == 'workflow_dispatch' and run.get('status') == 'completed'
            and run.get('conclusion') in ('success', 'failure', 'cancelled') and positive(run.get('run_attempt')),
            'Untrusted original candidate workflow')
    # A superseded UI observer can be cancelled only after every immutable build
    # and native/WSL gate below actually succeeded. No cancelled/skipped gate is
    # reusable; corrected installation/UI/migration evidence is always fresh.
    return checked_jobs(jobs)

def validate_revalidation_proof(proof, source, fixture=None, repository=None, run_id=None, assets=None):
    fields = {'schemaVersion','purpose','repository','sourceSha','fixtureSha','buildRunId','revalidationRunId',
              'artifactId','artifactName','artifactDigest','assetDigests','changes','requiredJobs'}
    require(isinstance(proof, dict) and set(proof) == fields, 'Invalid revalidation proof envelope')
    require(proof['schemaVersion'] == 1 and proof['purpose'] == 'candidate-fixture-revalidation', 'Invalid proof purpose')
    require(sha(source) and proof['sourceSha'] == source and sha(proof['fixtureSha']), 'Revalidation source mismatch')
    require(isinstance(proof['repository'],str) and re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+',proof['repository']), 'Invalid repository')
    if fixture is not None:
        require(proof['fixtureSha'] == fixture, 'Revalidation fixture mismatch')
    if repository is not None:
        require(proof['repository'] == repository, 'Revalidation repository mismatch')
    if run_id is not None:
        require(proof['revalidationRunId'] == run_id, 'Revalidation run mismatch')
    require(all(positive(proof[key]) for key in ('buildRunId','revalidationRunId','artifactId')),
            'Invalid revalidation run/artifact identity')
    require(proof['buildRunId'] != proof['revalidationRunId'], 'Revalidation must be a new workflow run')
    require(proof['artifactName'] == f"candidate-assembly-{proof['buildRunId']}"
            and isinstance(proof['artifactDigest'],str) and re.fullmatch(r'sha256:[a-f0-9]{64}',proof['artifactDigest']),
            'Invalid original assembly identity')
    require(isinstance(proof['assetDigests'],dict) and len(proof['assetDigests']) == 7
            and all(isinstance(name,str) and name and '/' not in name and '\\' not in name and digest(value)
                    for name,value in proof['assetDigests'].items()), 'Invalid original seven asset digests')
    require(isinstance(proof['changes'],list), 'Missing input comparison')
    names=[]
    for change in proof['changes']:
        require(isinstance(change,dict) and set(change)=={'path','before','after'} and change['path'] in ALLOWED_CHANGES,
                'Unreviewed input change in proof')
        names.append(change['path'])
        for value in (change['before'],change['after']):
            require(value is None or (isinstance(value,list) and len(value)==2 and value[0] in ('100644','100755') and sha(value[1])),
                    'Invalid input blob/mode receipt')
        require(change['before'] != change['after'], 'Unchanged input in change receipt')
    require(names == sorted(set(names)), 'Duplicate/unordered input changes')
    require(checked_jobs(proof['requiredJobs']) == proof['requiredJobs'], 'Reused jobs receipt mismatch')
    if assets is not None:
        expected={item['name']:item['digest'].removeprefix('sha256:') for item in assets}
        require(proof['assetDigests']==expected, 'Revalidated asset bytes mismatch')
    return proof

def bind_runtime(proof):
    require(os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
            and os.environ.get('GITHUB_REF') == 'refs/heads/main'
            and os.environ.get('GITHUB_WORKFLOW_REF') == f"{proof['repository']}/{WORKFLOW}@refs/heads/main"
            and os.environ.get('GITHUB_REPOSITORY') == proof['repository']
            and os.environ.get('GITHUB_SHA') == proof['fixtureSha']
            and os.environ.get('GITHUB_RUN_ID') == str(proof['revalidationRunId']),
            'Revalidation proof is not bound to this trusted main workflow execution')

def load_revalidation_proof(path, source, repository, run_id, assets=None):
    require(not path.is_symlink() and path.is_file(), 'Revalidation proof file missing')
    proof=validate_revalidation_proof(json.loads(path.read_text(encoding='utf-8')),source,
                                     repository=repository,run_id=run_id,assets=assets)
    bind_runtime(proof)
    require(proof['changes'] == verified_changes(source,proof['fixtureSha']), 'Revalidation input comparison changed')
    return proof
