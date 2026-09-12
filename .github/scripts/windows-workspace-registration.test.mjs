import assert from "node:assert/strict";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import {workspaceRequestExpression} from "./windows-workspace-registration.mjs";

test("native registration probe executes generated requests with the described context", async () => {
  for (const context of [null, {projectId:"project", worktreeId:"tree", revision:2, target:{kind:"windows"}}]) {
    const calls = [];
    const args = {root:String.raw`C:\한글 project\"quoted"`, nested:{value:")}; injected()"}};
    const result = await runInNewContext(workspaceRequestExpression("workspace.registry", "preview_windows", args), {
      window:{__TAURI_INTERNALS__:{invoke:async (command, input) => {
        calls.push({command, input});
        if (command === "plugin:product-shell|describe") return {handshake:{installationId:"installation", sessionId:"session"}, context};
        return "native-result";
      }}},
      crypto:{randomUUID:() => "request"},
      Date:{now:() => 1000},
    });
    assert.equal(result, "native-result");
    assert.equal(calls.length, 2);
    assert.equal(calls[1].command, "plugin:workspace|execute");
    const request = JSON.parse(JSON.stringify(calls[1].input.request));
    assert.deepEqual(request, {
      header:{protocolVersion:1, installationId:"installation", sessionId:"session", requestId:"request", deadlineMs:6000, route:"overview", ...(context ? {context} : {})},
      component:"workspace.registry", method:"preview_windows", args,
    });
  }
});

// node --check validates the harness itself, not JavaScript strings sent to
// Runtime.evaluate. Parse literals with the already-pinned Workspace compiler,
// then compile the actual decoded expression the browser will receive.
import {readFileSync} from "node:fs";
import {createRequire} from "node:module";
import {Script} from "node:vm";
const ts=createRequire(new URL("../../apps/devbox-workspace/package.json",import.meta.url))("typescript");
test("Workspace renderer probes contain valid decoded JavaScript expressions",()=>{
  let checked=0;
  for(const name of ["registration","definitions","dependencies","source","files","session-import","template-import","window-import","lsp","performance","runtime","runtime-import","runtime-wsl","runtime-crash"]){
    const filename=new URL(`./windows-workspace-${name}.mjs`,import.meta.url);
    const tree=ts.createSourceFile(filename.pathname,readFileSync(filename,"utf8"),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    const literal=node=>node&&(ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node));
    function visit(node){
      if(ts.isCallExpression(node)){
        const callee=node.expression;
        const argument=ts.isIdentifier(callee)&&callee.text==="waitForRenderer"?node.arguments[1]
          :ts.isPropertyAccessExpression(callee)&&callee.name.text==="evaluate"?node.arguments[0]:undefined;
        if(literal(argument)){
          const location=tree.getLineAndCharacterOfPosition(argument.getStart(tree));
          assert.doesNotThrow(()=>new Script(argument.text),`${filename.pathname}:${location.line+1}`);
          checked++;
        }
      }
      ts.forEachChild(node,visit);
    }
    visit(tree);
  }
  assert.ok(checked>=20,"the browser expression regression must inspect the actual fixture call sites");
});

test("long feature fixture requests select their own route inside the native deadline ceiling", async()=>{
  for (const [component,route] of [["workspace.dependencies","dependencies"],["workspace.source","source"],["workspace.lsp","files"]]) {
  let sent;
  await runInNewContext(workspaceRequestExpression(component,"fixture_method",{request:{path:"C:\\fixture"}}),{
    window:{__TAURI_INTERNALS__:{invoke:async(command,input)=>command==="plugin:product-shell|describe"?{handshake:{installationId:"installation",sessionId:"session"},context:null}:(sent=input.request)}},
    crypto:{randomUUID:()=>"request"},Date:{now:()=>1000},
  });
  assert.equal(sent.header.route,route);
  assert.equal(sent.header.deadlineMs,30000);
  }
});

test("native component rejection retains its structured problem in renderer diagnostics",async()=>{
  await assert.rejects(runInNewContext(workspaceRequestExpression("workspace.source","trust_status"),{
    window:{__TAURI_INTERNALS__:{invoke:async(command)=>{
      if(command==="plugin:product-shell|describe")return {handshake:{installationId:"installation",sessionId:"session"},context:null};
      throw {code:"unauthorized",provenance:{component:"workspace.source"}};
    }}},crypto:{randomUUID:()=>"request"},Date:{now:()=>1000},
  }),/Native Workspace request rejected:.*unauthorized/);
});

import {createWorkspaceLspProxy} from "./windows-workspace-lsp.mjs";
import {createConnection} from "node:net";
import {once} from "node:events";
test("owned LSP proxy holds then rejects requests without forwarding",async()=>{
  const proxy=await createWorkspaceLspProxy();
  const address=new URL(proxy.url);
  const socket=createConnection({host:address.hostname,port:Number(address.port)});
  try {
    await once(socket,"connect");proxy.hold();
    const chunks=[];socket.on("data",chunk=>chunks.push(chunk));
    socket.write("CONNECT registry.npmjs.org:443 HTTP/1.1\r\nHost: registry.npmjs.org:443\r\n\r\n");
    await proxy.waitForAttempt(0);assert.equal(chunks.length,0);
    const ended=once(socket,"end");proxy.release();await ended;
    assert.match(Buffer.concat(chunks).toString(),/^HTTP\/1\.1 502 Fixture Offline/);
    assert.equal(proxy.attempts(),1);
  } finally {socket.destroy();proxy.close();}
});

test("only pre-admission busy rejection retries with fresh IDs and a fixed context/deadline",async()=>{
  let sequence=0,now=1000;const attempts=[];
  const context={projectId:"p",worktreeId:"w",revision:3,target:{kind:"windows"}};
  const result=await runInNewContext(workspaceRequestExpression("workspace.lsp","save_lsp_config",{nativeRevision:"old"}),{
    window:{__TAURI_INTERNALS__:{invoke:async(command,input)=>{
      if(command==="plugin:product-shell|describe")return {handshake:{installationId:"i",sessionId:"s"},context};
      attempts.push(JSON.parse(JSON.stringify(input.request)));
      if(attempts.length<3)throw {code:"unavailable",provenance:{product:"workspace",component:"workspace.dispatch",requestId:"rejected",revision:1}};
      return {value:{issue:"lsp_config_changed"}};
    }}},crypto:{randomUUID:()=>String(++sequence)},Date:{now:()=>now},setTimeout:callback=>{now+=50;callback();},
  });
  assert.equal(result.value.issue,"lsp_config_changed");assert.equal(attempts.length,3);
  assert.deepEqual(attempts.map(request=>request.header.requestId),["1","2","3"]);
  for(const request of attempts){assert.deepEqual(request.header.context,context);assert.equal(request.header.deadlineMs,30000);assert.deepEqual(request.args,{nativeRevision:"old"});}
});
test("busy retries are bounded and never repeat accepted or ambiguous failures",async()=>{
  const busy={code:"unavailable",provenance:{product:"workspace",component:"workspace.dispatch",requestId:"rejected",revision:1}};
  for(const [failure,expected] of [[busy,20],[{...busy,provenance:{...busy.provenance,requestId:"accepted-request"}},1],[{code:"unauthorized"},1],[new Error("transport failed"),1]]){
    let calls=0;
    await assert.rejects(runInNewContext(workspaceRequestExpression("workspace.registry","apply_registration",{previewId:"one-use"}),{
      window:{__TAURI_INTERNALS__:{invoke:async command=>{if(command==="plugin:product-shell|describe")return {handshake:{installationId:"i",sessionId:"s"},context:null};calls++;throw failure;}}},
      crypto:{randomUUID:()=>String(calls)},Date:{now:()=>1000},setTimeout:callback=>callback(),
    }),/Native Workspace request rejected/);
    assert.equal(calls,expected);
  }
  let calls=0;
  const accepted={operation:{outcome:{state:"failed"}},value:{issue:"busy"}};
  assert.equal(await runInNewContext(workspaceRequestExpression("workspace.registry","apply_registration"),{
    window:{__TAURI_INTERNALS__:{invoke:async command=>{if(command==="plugin:product-shell|describe")return {handshake:{installationId:"i",sessionId:"s"},context:null};calls++;return accepted;}}},crypto:{randomUUID:()=>"request"},Date:{now:()=>1000},
  }),accepted);
  assert.equal(calls,1);
});

test("native editor trace observes IPC with Tauri's immutable invoke and preserves original responses", async () => {
  const {installWorkspaceEditorTrace} = await import("./windows-workspace-lsp.mjs");
  const originalInvoke=()=>"immutable", internals={};
  Object.defineProperty(internals,"invoke",{value:originalInvoke});
  const value={operation:{outcome:{state:"succeeded"}},value:{}};
  const fetch=async()=>new Response(JSON.stringify(value),{headers:{"content-type":"application/json"}});
  const window={fetch,__TAURI_INTERNALS__:internals};
  runInNewContext(`(${installWorkspaceEditorTrace.toString()})()`,{window,URL,performance});
  assert.equal(internals.invoke,originalInvoke);
  const response=await window.fetch("http://ipc.localhost/plugin%3Aworkspace%7Cexecute",{body:JSON.stringify({request:{component:"workspace.lsp",method:"save_lsp_document",args:{text:"synthetic private buffer"}}})});
  assert.deepEqual(await response.json(),value);
  const deadline=Date.now()+1000;
  while(window.__workspaceLspTrace.rows[0]?.phase==="pending"&&Date.now()<deadline)await new Promise(resolve=>setTimeout(resolve,5));
  const rows=JSON.parse(JSON.stringify(window.__workspaceLspTrace.rows));
  assert.equal(rows.length,1);assert.equal(rows[0].phase,"succeeded");assert.equal(rows[0].method,"save_lsp_document");
  assert.deepEqual(Object.keys(rows[0]).sort(),["elapsedMs","method","phase"]);
  assert.ok(!JSON.stringify(rows).includes("private"));
  window.__workspaceLspTrace.restore();assert.equal(window.fetch,fetch);
});

test("native editor trace ignores unrelated bodies and retains only static failure metadata", async () => {
  const {installWorkspaceEditorTrace} = await import("./windows-workspace-lsp.mjs");
  const value={operation:{outcome:{state:"failed"}},value:{issue:"file_snapshot_changed",private:"synthetic response"}};
  const window={fetch:async()=>new Response(JSON.stringify(value))};
  runInNewContext(`(${installWorkspaceEditorTrace.toString()})()`,{window,URL,performance});
  const body=JSON.stringify({request:{component:"workspace.files",method:"save_file",args:{text:"synthetic input"}}});
  await window.fetch("https://example.test/",{body});assert.equal(window.__workspaceLspTrace.rows.length,0);
  await window.fetch("http://ipc.localhost/plugin%3Aworkspace%7Cexecute",{body});
  const deadline=Date.now()+1000;
  while(window.__workspaceLspTrace.rows[0]?.phase==="pending"&&Date.now()<deadline)await new Promise(resolve=>setTimeout(resolve,5));
  const row=window.__workspaceLspTrace.rows[0];assert.equal(row.phase,"failed");assert.equal(row.issue,"file_snapshot_changed");
  assert.ok(!JSON.stringify(row).includes("synthetic"));window.__workspaceLspTrace.restore();
});


test("bounded writer-wait fixtures retain their short original deadline", async () => {
  let sent;
  await runInNewContext(workspaceRequestExpression("workspace.files", "save_file", {request:{path:"C:\\fixture"}}, 500), {
    window:{__TAURI_INTERNALS__:{invoke:async(command,input)=>command==="plugin:product-shell|describe"?{handshake:{installationId:"installation",sessionId:"session"},context:null}:(sent=input.request)}},
    crypto:{randomUUID:()=>"request"}, Date:{now:()=>1000},
  });
  assert.equal(sent.header.deadlineMs,1500);
  assert.equal(sent.header.route,"files");
  for(const budget of [0,99,29001,Infinity,"500"]) assert.throws(()=>workspaceRequestExpression("workspace.files","save_file",{},budget));
});
