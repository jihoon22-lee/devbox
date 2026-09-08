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
