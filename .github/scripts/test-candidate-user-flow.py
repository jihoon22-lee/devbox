import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from candidate_user_flow import verify_user_flow_evidence
from candidate_revalidation import REQUIRED_JOBS

class UserFlowCandidateTests(unittest.TestCase):
    def test_requires_every_bound_scenario_and_real_unchanged_screenshot(self):
        matrix=json.loads((Path(__file__).parent/'suite-user-flow-matrix.json').read_text())
        source='a'*40
        assets=[{'name':'owned.zip','digest':'sha256:'+'b'*64}]
        digests={'owned.zip':'b'*64}
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            screenshot=root/'user-flow-files/screenshot.png'
            screenshot.parent.mkdir()
            screenshot.write_bytes(b'\x89PNG\r\n\x1a\nsynthetic validator fixture')
            report=root/'user-flows.json'
            evidence={'schemaVersion':1,'installationKey':'d'*64,'expectedSource':source,'expectedFixture':source,'expectedDigests':digests,
                'screenshots':{'screenshot.png':hashlib.sha256(screenshot.read_bytes()).hexdigest()},
                'results':[{'id':row['id'],'installationKey':'d'*64,'status':'PASS','sourceSha':source,'fixtureSha':source,'artifactDigests':digests,'evidenceKind':row['evidenceKind'],'failureCode':None,'assertions':['synthetic validator observation'],'screenshotPaths':['screenshot.png']} for row in matrix]}
            def verify(value):
                report.write_text(json.dumps(value))
                return verify_user_flow_evidence(report,source,assets)
            self.assertEqual(verify(evidence)['requiredScenarios'],len(matrix))
            for index in range(len(matrix)):
                missing=copy.deepcopy(evidence); missing['results'].pop(index)
                with self.assertRaises(ValueError): verify(missing)
            for field,value in [('installationKey','e'*64),('status','NOT_RUN'),('sourceSha','c'*40),('fixtureSha','c'*40),('evidenceKind','native-boundary'),('artifactDigests',{}),('assertions',[]),('screenshotPaths',[])]:
                bad=copy.deepcopy(evidence); bad['results'][0][field]=value
                with self.subTest(field=field),self.assertRaises(ValueError): verify(bad)
            legacy=copy.deepcopy(evidence)
            row=next(row for row in legacy['results'] if row['id']=='DELIVERY-01')
            row.update(installationKey='e'*64,fixtureKind='legacy-upgrade',parentInstallationKey='d'*64)
            self.assertEqual(verify(legacy)['requiredScenarios'],len(matrix))
            for field,value in [('fixtureKind','other'),('parentInstallationKey','f'*64),('installationKey','invalid')]:
                rejected=copy.deepcopy(legacy)
                next(row for row in rejected['results'] if row['id']=='DELIVERY-01')[field]=value
                with self.subTest(legacy_field=field),self.assertRaises(ValueError): verify(rejected)
            revalidated=copy.deepcopy(evidence)
            fixture='f'*40
            new_assets=[{'name':f'asset-{i}','digest':'sha256:'+str(i)*64} for i in range(7)]
            digest_map={a['name']:a['digest'][7:] for a in new_assets}
            revalidated.update(expectedFixture=fixture,expectedDigests=digest_map)
            for row in revalidated['results']: row.update(fixtureSha=fixture,artifactDigests=digest_map)
            proof=dict(schemaVersion=1,purpose='candidate-fixture-revalidation',repository='owner/repo',
                sourceSha=source,fixtureSha=fixture,buildRunId=11,revalidationRunId=22,artifactId=33,
                artifactName='candidate-assembly-11',artifactDigest='sha256:'+'b'*64,assetDigests=digest_map,changes=[],
                requiredJobs=[dict(id=i+1,name=name,status='completed',conclusion='success') for i,name in enumerate(REQUIRED_JOBS)])
            report.write_text(json.dumps(revalidated))
            with self.assertRaises(ValueError):verify_user_flow_evidence(report,source,new_assets)
            self.assertEqual(verify_user_flow_evidence(report,source,new_assets,proof)['requiredScenarios'],len(matrix))
            for target in [revalidated,revalidated['results'][0]]:
                target['diagnosticOnly']=True
                report.write_text(json.dumps(revalidated))
                with self.assertRaises(ValueError):verify_user_flow_evidence(report,source,new_assets,proof)
                del target['diagnosticOnly']
            screenshot.write_bytes(b'changed')
            with self.assertRaises(ValueError): verify(evidence)

if __name__=='__main__': unittest.main()
