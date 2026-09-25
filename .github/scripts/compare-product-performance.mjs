// Compare only measurements collected on this same disposable candidate VM.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
const json=file=>JSON.parse(readFileSync(file,'utf8').replace(/^\uFEFF/,''));
const scope=process.argv[2];
const groups={
 'product-shells':[['workspace','workbench','workspace-performance.json'],['control-center','devbox-manager','control-center-performance.json']],
 api:[['api-studio','api-playground','api-lifecycle.json']],
 knowledge:[['knowledge','knowledge-base','knowledge-lifecycle.json'],['knowledge.search','everything-plus','knowledge-lifecycle.json']],
};
assert.ok(Object.hasOwn(groups,scope));
const baseline=json('performance-baseline/runtime.json');
assert.equal(baseline.scope,'selected-pinned-baseline-measurements');
assert.equal(baseline.summary.failed,0);assert.equal(baseline.summary.unattempted,0);assert.equal(baseline.runtimeRootRemoved,true);
const comparisons=[];
for(const [product,anchor,file] of groups[scope]){
 const prior=baseline.apps.find(app=>app.id===anchor);
 assert.ok(prior?.performance);assert.equal(prior.cleanup.dataRestored,true);
 const record=json(`product-foundation-evidence/${file}`), current=record.performance??record;
 assert.deepEqual(current.host,baseline.performanceHost,'candidate and anchor must be from this same VM');
 assert.equal(current.budget.passed,true);
 const fields=['coldRendererReadyMs','firstKeyboardEventMs','warmExistingWindowMs'];
 const values={};
 for(const field of fields){assert.ok(Number.isFinite(current[field])&&Number.isFinite(prior.performance[field]));values[field]={baseline:prior.performance[field],candidate:current[field]};}
 for(const field of ['cpuPercentOfMachine','workingSetSumBytes','processCount'])values[field]={baseline:prior.performance.idle[field],candidate:current.idle[field]};
 comparisons.push({product,anchor,values,baselineWorkload:prior.performance.workload??null,candidateWorkload:current.workload??null,conditions:current.conditions});
}
writeFileSync(`product-foundation-evidence/performance-comparison-${scope}.json`,JSON.stringify({source:process.env.GITHUB_SHA,host:baseline.performanceHost,baselineCommit:baseline.expectedCommit,comparisons,scope:'same-host measurements and unchanged B01 budgets; configured profiles/workloads are labelled, not assumed identical',knownBaselineFailures:baseline.performanceSummary.knownBaselineFailures,result:'passed'},null,2));
