import { describe, expect, it } from "vitest";
import { runtimeDestination, runtimeLogRequest, runtimeDiagnostic, terminalLogRequest } from "./runtimeNavigation";
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

describe("Runtime diagnostic navigation", () => {
  const diagnostic = {id:"a".repeat(32),runId:"run-1",revision:"b".repeat(64),context,fromRoute:"tasks",relativePath:"src/main.rs",line:12,column:3};
  it("retains the verified file range", () => {
    expect(runtimeDiagnostic(diagnostic,context,"tasks")).toEqual({id:diagnostic.id,relativePath:"src/main.rs",line:12,column:3});
  });
  it("discards stale destinations and malformed ranges", () => {
    expect(runtimeDiagnostic(diagnostic,{...context,projectId:"other"},"tasks")).toBeNull();
    expect(runtimeDiagnostic(diagnostic,context,"logs")).toBeNull();
    for (const relativePath of ["../outside.rs","/absolute.rs","C:/outside.rs","src/../outside.rs","src\\outside.rs"]) expect(runtimeDiagnostic({...diagnostic,relativePath},context,"tasks")).toBeNull();
    for (const column of [0,-1,0.5,"3",undefined]) expect(runtimeDiagnostic({...diagnostic,column},context,"tasks")).toBeNull();
  });
});


describe("one-time Terminal Logs requests", () => {
  const request={id:"c".repeat(32),context,source:{kind:"wslFile",distro:"Ubuntu",path:"/var/log/synthetic.log"}};
  it("accepts only the native current-context adapter and normalizes journal null",()=>{
    expect(terminalLogRequest(request,context)).toEqual({id:request.id,source:request.source});
    expect(terminalLogRequest({...request,context:null,source:{kind:"wslJournal",distro:"Ubuntu",unit:null}},null)?.source).toEqual({kind:"wslJournal",distro:"Ubuntu"});
  });
  it("rejects late project requests, traversal and extra execution fields",()=>{
    expect(terminalLogRequest(request,{...context,revision:2})).toBeNull();
    expect(terminalLogRequest({...request,source:{...request.source,path:"/var/../private"}},context)).toBeNull();
    expect(terminalLogRequest({...request,source:{...request.source,command:"cat file"}},context)).toBeNull();
    expect(terminalLogRequest({...request,source:{kind:"wslJournal",distro:"Ubuntu",unit:"--help"}},context)).toBeNull();
  });
});
