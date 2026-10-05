import copy
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from candidate_revalidation import REQUIRED_JOBS, validate_revalidation_proof, validate_original_run, compare_inputs, bind_runtime

SOURCE = 'a' * 40
FIXTURE = 'b' * 40
REPO = 'owner/repo'

def proof():
    return dict(schemaVersion=1, purpose='candidate-fixture-revalidation', repository=REPO,
                sourceSha=SOURCE, fixtureSha=FIXTURE, buildRunId=11, revalidationRunId=22,
                artifactId=33, artifactName='candidate-assembly-11', artifactDigest='sha256:'+'c'*64,
                assetDigests={str(i):'d'*64 for i in range(7)}, changes=[],
                requiredJobs=[dict(id=i+1,name=name,status='completed',conclusion='success') for i,name in enumerate(REQUIRED_JOBS)])

class RevalidationTests(unittest.TestCase):
    def test_proof_identity_and_required_success(self):
        p=proof()
        self.assertEqual(validate_revalidation_proof(p,SOURCE,FIXTURE,REPO,22),p)
        for key,value in [('fixtureSha',SOURCE),('sourceSha',FIXTURE),('revalidationRunId',11),('repository','fork/repo'),('artifactDigest','bad'),('assetDigests',{}),('diagnosticOnly',True)]:
            bad={**p,key:value}
            with self.subTest(key=key),self.assertRaises(ValueError): validate_revalidation_proof(bad,SOURCE,FIXTURE,REPO,22)
        for status in ['failure','skipped',None]:
            bad=copy.deepcopy(p);bad['requiredJobs'][0]['conclusion']=status
            with self.assertRaises(ValueError):validate_revalidation_proof(bad,SOURCE,FIXTURE,REPO,22)

    def test_runtime_binding_rejects_other_workflow_branch_source_and_run(self):
        p=proof()
        env={'GITHUB_EVENT_NAME':'workflow_dispatch','GITHUB_REF':'refs/heads/main',
             'GITHUB_WORKFLOW_REF':f'{REPO}/.github/workflows/windows-candidate-revalidation.yml@refs/heads/main',
             'GITHUB_REPOSITORY':REPO,'GITHUB_SHA':FIXTURE,'GITHUB_RUN_ID':'22'}
        with patch.dict(os.environ,env,clear=True): bind_runtime(p)
        for key,value in [('GITHUB_SHA',SOURCE),('GITHUB_RUN_ID','11'),('GITHUB_REF','refs/heads/other'),('GITHUB_EVENT_NAME','pull_request'),('GITHUB_WORKFLOW_REF','other')]:
            with self.subTest(key=key),patch.dict(os.environ,{**env,key:value},clear=True),self.assertRaises(ValueError):bind_runtime(p)

    def test_failed_overall_run_requires_every_reused_job_success(self):
        run=dict(id=11,path='.github/workflows/windows-package-candidate.yml',event='workflow_dispatch',status='completed',conclusion='failure',head_sha=SOURCE,head_branch='main',head_repository={'full_name':REPO},run_attempt=1)
        jobs=proof()['requiredJobs']
        validate_original_run(run,jobs,REPO,SOURCE,11)
        for key,value in [('head_repository',{'full_name':'fork/repo'}),('head_sha',FIXTURE),('event','pull_request'),('status','in_progress')]:
            with self.assertRaises(ValueError):validate_original_run({**run,key:value},jobs,REPO,SOURCE,11)
        with self.assertRaises(ValueError):validate_original_run(run,jobs[:-1],REPO,SOURCE,11)

    def test_input_comparison_rejects_product_lock_build_mode_and_unlisted_changes(self):
        base={'apps/product.rs':('100644','a'*40),'Cargo.lock':('100644','b'*40),'.github/scripts/windows-suite-direct-layout.mjs':('100644','c'*40)}
        allowed={**base,'.github/scripts/windows-suite-direct-layout.mjs':('100644','d'*40)}
        self.assertEqual(len(compare_inputs(base,allowed)),1)
        for path in ['apps/product.rs','Cargo.lock','.github/scripts/build-windows-packages.ps1','unknown.txt']:
            bad={**base,path:('100644','e'*40)}
            with self.subTest(path=path),self.assertRaises(ValueError):compare_inputs(base,bad)
        with self.assertRaises(ValueError):compare_inputs(base,{**base,'apps/product.rs':('100755','a'*40)})

if __name__=='__main__': unittest.main()
