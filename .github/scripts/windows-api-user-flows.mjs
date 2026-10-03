import assert from "node:assert/strict";
import path from "node:path";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { createApiUserFlowContext } from "./windows-api-user-flow-adapter.mjs";
import { writeUserFlowResults } from "./suite-user-flow-results.mjs";
import { observeProductPerformance } from "./windows-suite-layout.mjs";
import { summarizeEvidence } from "./suite-user-flow-evidence.mjs";

export async function runApiUserFlows() {
  const context=await createApiUserFlowContext();
  const matrix=JSON.parse(await readFile(new URL("./suite-user-flow-matrix.json",import.meta.url),"utf8"))
    .filter(row=>/^windows-api-(http-semantics|mcp-auth|environments|webhooks|grpc|transforms)\.mjs$/.test(row.module));
  assert.equal(matrix.length,11,"API matrix scenario registration incomplete");
  const results=[];
  try {
    for(const module of new Set(matrix.map(row=>row.module))) {
      const runner=await import(new URL(module,import.meta.url));
      if(module==="windows-api-http-semantics.mjs") {
        await observeProductPerformance({product:"api-studio",cdp:context.cdp,getIdentities:context.getIdentities,coldRendererReadyMs:context.coldRendererReadyMs,warmExistingWindowMs:context.warmExistingWindowMs,workload:async()=>{
          results.push(...await runner.run(context));
          assert.equal(results.length,3,"Representative HTTP scenarios incomplete");
          assert.ok(results.every(row=>row.status==="PASS"),"Representative HTTP task failed");
          assert.ok(Number.isFinite(context.httpCompletedMs),"Native HTTP completion latency unobserved");
          return {taskCompleted:true,completeMs:context.httpCompletedMs,result:"passed"};
        }});
      } else results.push(...await runner.run(context));
      if(results.some(result=>result.status!=="PASS")) break;
    }
  } finally {
    try { await context.ui.closeOwnedWindow(); await context.close(); }
    catch {
      await context.close().catch(()=>{});
      if(results.length) {
        const last=results.at(-1);
        last.status="FAIL";
        last.failureCode="api-owned-process-cleanup-failed";
      }
    }
    if(results.length) await writeUserFlowResults("api",results);
  }
  const summary=summarizeEvidence(matrix,results,{expectedSource:context.sourceSha,expectedFixture:context.fixtureSha,expectedDigests:context.artifactDigests});
  assert.equal(summary.ready,true,JSON.stringify(summary));
  return summary;
}
if(process.argv[1] && import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await runApiUserFlows()));
}
