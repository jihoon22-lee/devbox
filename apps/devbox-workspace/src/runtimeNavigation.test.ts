import { describe, expect, it } from "vitest";
import { runtimeDestination, runtimeLogRequest } from "./runtimeNavigation";
const context = {projectId:"project-1",worktreeId:"tree-1",revision:1,target:{kind:"windows" as const}};
const log = {id:"a".repeat(32),fromRoute:"tasks",context,source:{kind:"runtimeRun",runId:"run-1",stream:"stdout",revision:"b".repeat(64)}};
describe("Runtime navigation", () => {
  it("accepts the current native run reference independent of object key order", () => {
    expect(runtimeLogRequest({...log,context:{target:context.target,revision:1,worktreeId:"tree-1",projectId:"project-1"}},context,"tasks")?.source.runId).toBe("run-1");
  });
  it("rejects late view/project changes and path-bearing references", () => {
    expect(runtimeLogRequest(log,context,"files")).toBeNull();
    expect(runtimeLogRequest(log,{...context,revision:2},"tasks")).toBeNull();
    expect(runtimeLogRequest({...log,source:{...log.source,path:"C:/private/data.db"}},context,"tasks")).toBeNull();
    expect(runtimeLogRequest({...log,source:{...log.source,runId:"../other"}},context,"tasks")).toBeNull();
    expect(runtimeLogRequest({...log,source:{...log.source,revision:"old"}},context,"tasks")).toBeNull();
  });
  it("only accepts the closed owning-task or project destination", () => {
    const event={route:"tasks",fromRoute:"runtime",context:null};
    expect(runtimeDestination(event,null,"runtime")).toBe("tasks");
    expect(runtimeDestination({...event,route:"shell"},null,"runtime")).toBeNull();
    expect(runtimeDestination(event,null,"source")).toBeNull();
    expect(runtimeDestination({...event,path:"untrusted"},null,"runtime")).toBeNull();
  });
});
