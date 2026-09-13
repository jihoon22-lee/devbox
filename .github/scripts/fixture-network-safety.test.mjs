import test from "node:test";
import assert from "node:assert/strict";
import {requireHostedNetworkFixture} from "./fixture-network-safety.mjs";
const hosted={GITHUB_ACTIONS:"true",RUNNER_ENVIRONMENT:"github-hosted",RUNNER_OS:"Windows",GITHUB_REPOSITORY:"jihoon22-lee/devbox",GITHUB_RUN_ID:"123"};
test("network fixtures reject local and self-hosted invocations before any setup",()=>{
 for(const env of [{},{...hosted,GITHUB_ACTIONS:undefined},{...hosted,RUNNER_ENVIRONMENT:"self-hosted"},{...hosted,RUNNER_OS:"Linux"},{...hosted,GITHUB_REPOSITORY:"another/repository"},{...hosted,GITHUB_RUN_ID:""}])assert.throws(()=>requireHostedNetworkFixture(env),/disabled on local and self-hosted/);
});
test("the disposable hosted runner keeps its own run identity",()=>assert.deepEqual(requireHostedNetworkFixture(hosted),{runId:"123"}));
