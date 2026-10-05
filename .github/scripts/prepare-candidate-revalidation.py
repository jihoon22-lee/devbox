#!/usr/bin/env python3
"""Authenticate retained candidate inputs; never relabel or rebuild package bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from candidate_revalidation import (bind_runtime, require, sha, positive, verified_changes,
                                    validate_original_run, validate_revalidation_proof)
from suite_release_contract import manifest_assets


def api(endpoint):
    result=subprocess.run(['gh','api','--method','GET',endpoint],capture_output=True,text=True,check=False)
    require(result.returncode == 0, f'Authenticated GitHub lookup failed: {endpoint}')
    return json.loads(result.stdout)


def select_artifact(artifacts, name, repository_id, source, run_id):
    found=[item for item in artifacts if item.get('name')==name]
    require(len(found)==1, f'Missing or duplicate original artifact: {name}')
    item=found[0]
    owner=item.get('workflow_run',{})
    require(positive(item.get('id')) and item.get('expired') is False and positive(item.get('size_in_bytes'))
            and owner.get('id')==run_id and owner.get('head_sha')==source and owner.get('head_branch')=='main'
            and owner.get('repository_id')==repository_id and owner.get('head_repository_id')==repository_id,
            f'Untrusted/expired original artifact: {name}')
    require(isinstance(item.get('digest'),str) and item['digest'].startswith('sha256:')
            and len(item['digest'])==71 and all(c in '0123456789abcdef' for c in item['digest'][7:]),
            'Original artifact missing immutable digest')
    return item


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository',required=True)
    parser.add_argument('--source')
    parser.add_argument('--fixture',required=True)
    parser.add_argument('--build-run',type=int,required=True)
    parser.add_argument('--run',type=int,required=True)
    parser.add_argument('--tag')
    parser.add_argument('--resolve',action='store_true')
    parser.add_argument('--github-output',type=Path)
    parser.add_argument('--assets',type=Path)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    require(sha(args.fixture) and positive(args.build_run) and positive(args.run), 'Invalid requested identities')
    bind_runtime(dict(repository=args.repository,fixtureSha=args.fixture,revalidationRunId=args.run))
    run=api(f'repos/{args.repository}/actions/runs/{args.build_run}')
    source=args.source or run.get('head_sha')
    require(sha(source), 'Missing exact original source')
    jobs=api(f'repos/{args.repository}/actions/runs/{args.build_run}/attempts/{run["run_attempt"]}/jobs?per_page=100&page=1')
    require(jobs.get('total_count')==len(jobs.get('jobs',[])), 'Incomplete original job listing')
    reused=validate_original_run(run,jobs.get('jobs'),args.repository,source,args.build_run)
    changes=verified_changes(source,args.fixture)
    artifacts=api(f'repos/{args.repository}/actions/runs/{args.build_run}/artifacts?per_page=100&page=1')
    require(artifacts.get('total_count')==len(artifacts.get('artifacts',[])), 'Incomplete original artifact listing')
    repository_id=run.get('repository',{}).get('id')
    require(positive(repository_id), 'Original repository ID missing')
    selection={}
    for key,name in [('artifact',f'candidate-assembly-{args.build_run}'),
                     ('windows_lsp_artifact',f'candidate-private-windows-lsp-{args.build_run}'),
                     ('wsl_lsp_artifact',f'candidate-private-wsl-lsp-{args.build_run}')]:
        selection[key]=select_artifact(artifacts['artifacts'],name,repository_id,source,args.build_run)
    artifact=selection['artifact']
    outputs=dict(build_source=source,build_run=args.build_run,artifact_id=artifact['id'],assembly_artifact_id=artifact['id'],
                 artifact_name=artifact['name'],artifact_digest=artifact['digest'],
                 windows_lsp_artifact_id=selection['windows_lsp_artifact']['id'],
                 wsl_lsp_artifact_id=selection['wsl_lsp_artifact']['id'])
    if args.github_output:
        with args.github_output.open('a',encoding='utf-8') as out:
            out.writelines(f'{key}={value}\n' for key,value in outputs.items())
    if args.resolve:
        print(json.dumps(outputs,sort_keys=True))
        return
    require(args.assets is not None and args.output is not None, 'Proof creation requires downloaded assets and output')
    root=args.assets.resolve(strict=True)
    manifest=json.loads((root/'release-manifest.json').read_text(encoding='utf-8'))
    expected=set(manifest_assets(manifest,args.tag,source))|{'release-manifest.json'}
    files=list(root.iterdir())
    require(len(files)==7 and {item.name for item in files}==expected
            and all(item.is_file() and not item.is_symlink() for item in files),'Original seven candidate files required')
    asset_digests={item.name:hashlib.sha256(item.read_bytes()).hexdigest() for item in files}
    original=json.loads((root.parent/'evidence/candidate-metadata.json').read_text(encoding='utf-8'))
    require(original.get('artifactKind')=='candidate' and original.get('targetCommit')==source
            and original.get('repository')==args.repository and original.get('workflowRun')==args.build_run,
            'Downloaded original assembly metadata identity mismatch')
    original_digests={item['name']:item['digest'].removeprefix('sha256:') for item in original['assets']}
    require(asset_digests==original_digests,'Downloaded original asset digest mismatch')
    proof=dict(schemaVersion=1,purpose='candidate-fixture-revalidation',repository=args.repository,
               sourceSha=source,fixtureSha=args.fixture,buildRunId=args.build_run,revalidationRunId=args.run,
               artifactId=artifact['id'],artifactName=artifact['name'],artifactDigest=artifact['digest'],
               assetDigests=asset_digests,changes=changes,requiredJobs=reused)
    validate_revalidation_proof(proof,source,args.fixture,args.repository,args.run)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with args.output.open('x',encoding='utf-8') as output:
        json.dump(proof,output,indent=2,sort_keys=True);output.write('\n')
    print(f'Revalidation provenance prepared: build {source}, fixture {args.fixture}; seven original files unchanged')

if __name__=='__main__':
    main()
