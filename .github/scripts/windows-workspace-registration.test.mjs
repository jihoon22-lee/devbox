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
  for(const name of ["registration","definitions","dependencies","files"]){
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

test("dependency fixture requests select their own route and bounded analysis deadline", async()=>{
  let sent;
  await runInNewContext(workspaceRequestExpression("workspace.dependencies","dependency_inventory",{request:{path:"C:\\fixture"}}),{
    window:{__TAURI_INTERNALS__:{invoke:async(command,input)=>command==="plugin:product-shell|describe"?{handshake:{installationId:"installation",sessionId:"session"},context:null}:(sent=input.request)}},
    crypto:{randomUUID:()=>"request"},Date:{now:()=>1000},
  });
  assert.equal(sent.header.route,"dependencies");
  assert.equal(sent.header.deadlineMs,31000);
});
