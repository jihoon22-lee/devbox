// S03: real native loopback capture/request/DPAPI/transform/diff/mock/Knowledge.
// Only disposable GitHub-hosted Windows processes and synthetic data are used.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { Cdp, unusedPort, waitForCdp, windowsProcessIsElevated, inspectElevatedCdpPolicy, installElevatedCdpPolicy, restoreElevatedCdpPolicy } from "./windows-packaged-smoke.mjs";
assert.equal(process.platform, "win32"); assert.equal(process.env.GITHUB_ACTIONS, "true"); assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
const root = mkdtempSync(path.join(tmpdir(), "devbox-api-migration-fixture-s03-"));
const executable = path.join(root, `api-s03-${randomUUID()}.exe`); copyFileSync(path.resolve("target/debug/devbox-api-studio.exe"), executable);
const profile = path.join(root, "webview");
const secret = `s03-fixture-${randomUUID()}`;
const evidence = { source: process.env.GITHUB_SHA, environment: "github-hosted-windows", step: "start", result: "failed" };
mkdirSync("product-foundation-evidence", { recursive: true });
function progress(step) { evidence.step = step; writeFileSync("product-foundation-evidence/api-workflow.json", JSON.stringify(evidence, null, 2)); }
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
let hits = 0, credentialMatched = false, requestBodyMatched = false;
const server = createServer(async (request, response) => {
  const chunks = []; for await (const chunk of request) chunks.push(chunk);
  hits++; credentialMatched = request.headers.authorization === `Bearer ${secret}`;
  requestBodyMatched = Buffer.concat(chunks).toString() === '{"fixture":true}';
  response.statusCode = credentialMatched ? 201 : 401; response.setHeader("Content-Type", "application/json");
  response.end(JSON.stringify({ fixture: "s03-response", echo: secret, value: 2 }));
});
server.listen(0, "127.0.0.1"); await once(server, "listening");
const endpoint = `http://127.0.0.1:${server.address().port}/s03`;
const live = new Set(), policies = new Set(); let ui;
async function until(check, label, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await check()) return; await delay(100); }
  throw new Error(label);
}
async function wait(expression, label, timeout) { await until(() => ui.cdp.evaluate(expression), label, timeout); }
async function click(scope, label) {
  const expression = `Array.from(document.querySelectorAll(${JSON.stringify(scope + " button")})).find(button => button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled)`;
  await wait(`!!(${expression})`, "workflow control not ready");
  await ui.cdp.evaluate(`(${expression}).click()`);
}
async function input(selector, value) {
  await ui.cdp.evaluate(`(() => {const input=document.querySelector(${JSON.stringify(selector)});if(!input)throw new Error("fixture input missing");const prototype=input instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(prototype,"value").set.call(input,${JSON.stringify(value)});input.dispatchEvent(new Event("input",{bubbles:true}));})()`);
}
async function call(component, method, args = {}) {
  const route = component === "api-studio.webhooks" ? "webhooks" : component === "api-studio.transforms" ? "transforms" : "requests";
  return ui.cdp.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const d=await invoke("plugin:product-shell|describe");return invoke("plugin:api-studio|execute",{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)}},component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}});})()`);
}
async function success(component, method, args) { const result = await call(component, method, args); assert.equal(result.operation.outcome.state, "succeeded", method); return result.value; }
async function start() {
  const port = await unusedPort(); const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
  if (policy) { installElevatedCdpPolicy(policy); policies.add(policy); }
  const env = { ...process.env, DEVBOX_API_MIGRATION_FIXTURE_ROOT: root, WEBVIEW2_USER_DATA_FOLDER: profile, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
  const child = spawn(executable, [], { env, stdio: "ignore", windowsHide: true }); live.add(child); await once(child, "spawn");
  const target = await waitForCdp(port, "Devbox API Studio"); const cdp = new Cdp(target.webSocketDebuggerUrl); await cdp.connect(); await cdp.send("Page.enable");
  ui = { child, cdp, policy }; await wait('!!document.querySelector(".url-input")', "API startup did not finish");
}
async function stop() {
  const item = ui; if (!item) return;
  await call("api-studio.webhooks", "quit_product").catch(() => undefined);
  await until(() => item.child.exitCode !== null || item.child.signalCode !== null, "product did not quit"); assert.equal(item.child.exitCode, 0);
  item.cdp.close(); live.delete(item.child); if (item.policy) { restoreElevatedCdpPolicy(item.policy); policies.delete(item.policy); } ui = null;
}
try {
  await start(); assert.equal((await success("api-studio.webhooks", "server_status")).running, false); assert.equal(hits, 0);
  // Set up only an empty secret reference. The real user-facing Change action
  // below supplies the value to native DPAPI; plaintext is never seeded in storage.
  await ui.cdp.evaluate('localStorage.setItem("apip-environments",JSON.stringify({version:1,environments:[{id:"s03-env",name:"S03 fixture",variables:[{key:"WEBHOOK_SECRET",value:"",secret:true}]}]}))');
  await ui.cdp.send("Page.reload");
  await wait('!!Array.from(document.querySelectorAll(".env-name")).find(button=>button.textContent==="S03 fixture")', "empty environment fixture did not load");
  await click(".api-feature-requests .env-item", "S03 fixture");
  await wait('!!document.querySelector(".env-var-secret.unconfigured")', "explicit environment selection did not expose the empty reference");
  progress("capture");
  const capturePort = await unusedPort(); await success("api-studio.webhooks", "start_server", { bind: "127.0.0.1", port: capturePort, allowLan: false });
  const captured = await fetch(`http://127.0.0.1:${capturePort}/s03`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${secret}` }, body: '{"fixture":true}' }); await captured.arrayBuffer();
  const captures = await success("api-studio.webhooks", "list_history"); assert.equal(captures.length, 1); assert.ok(!JSON.stringify(captures).includes(secret));
  const logs = await call("api-studio.webhooks", "send_history_to_log_lens", { historyId: captures[0].id });
  assert.equal(logs.operation.outcome.state, "failed"); assert.equal(logs.value.issue, "Logs 연결을 사용할 수 없습니다. 수신 요청과 저장한 fixture는 유지됩니다.");
  assert.equal((await success("api-studio.webhooks", "list_history")).length, 1);
  await success("api-studio.webhooks", "stop_server");
  progress("request-preview"); await success("api-studio.webhooks", "send_history_to_api", { historyId: captures[0].id });
  await wait('!!document.querySelector(".api-feature-requests:not([hidden]) .handoff-dialog")', "capture preview did not open");
  assert.equal(hits, 0); assert.equal(await ui.cdp.evaluate('document.querySelector(".url-input").value'), "");
  await click(".api-feature-requests .handoff-dialog", "적용");
  await wait('document.querySelector(".url-input").value === "/s03" && !document.querySelector(".handoff-dialog")', "explicit capture apply failed"); assert.equal(hits, 0);
  progress("credential-reconnect"); let promptAnswered = false, promptError = false;
  const answerPrompt = event => {
    const message = JSON.parse(String(event.data)); if (message.method !== "Page.javascriptDialogOpening") return;
    const expected = message.params?.type === "prompt" && message.params?.message === "WEBHOOK_SECRET 새 값 입력";
    promptAnswered = expected; promptError = !expected;
    void ui.cdp.send("Page.handleJavaScriptDialog", { accept: expected, ...(expected ? { promptText: secret } : {}) }).catch(() => { promptError = true; });
  };
  ui.cdp.socket.addEventListener("message", answerPrompt);
  await click(".api-feature-requests .env-var-row", "변경");
  await wait('!!document.querySelector(".env-var-secret:not(.unconfigured)")', "native DPAPI credential was not saved");
  ui.cdp.socket.removeEventListener("message", answerPrompt); assert.equal(promptAnswered, true); assert.equal(promptError, false);
  assert.equal(await ui.cdp.evaluate(`localStorage.getItem("apip-environments").includes(${JSON.stringify(secret)})`), false);
  await click(".api-feature-requests .tabs", "HEADERS");
  const headerIndex = await ui.cdp.evaluate('Array.from(document.querySelectorAll(".header-row")).findIndex(row=>row.querySelector("input[placeholder=\\"헤더 이름\\"]").value.toLowerCase()==="authorization")'); assert.ok(headerIndex >= 0);
  await input(`.header-row:nth-child(${headerIndex + 1}) input[placeholder="Value 또는 \${SECRET_NAME}"]`, "Bearer ${WEBHOOK_SECRET}");
  await input(".url-input", endpoint); assert.equal(hits, 0);
  progress("explicit-send"); await click(".api-feature-requests .request-bar", "보내기");
  await until(() => hits === 1, "explicit request was not sent"); await wait('!!document.querySelector(".resp-body")?.textContent.includes("s03-response")', "native response did not render");
  assert.equal(credentialMatched, true); assert.equal(requestBodyMatched, true);
  const masked = await ui.cdp.evaluate('document.querySelector(".resp-body").textContent'); assert.ok(masked.includes("[REDACTED]")); assert.ok(!masked.includes(secret));
  progress("response-transform");
  await ui.cdp.evaluate('(()=>{const body=document.querySelector(".resp-body");const range=document.createRange();range.selectNodeContents(body);const selection=getSelection();selection.removeAllRanges();selection.addRange(range);})()');
  await click(".api-feature-requests .response-actions", "선택 영역을 Developer Toolbox로 보내기");
  await wait('!!document.querySelector(".api-feature-transforms:not([hidden]) [role=dialog]")', "response transform preview did not open");
  await click(".api-feature-transforms [role=dialog]", "적용");
  await wait('document.querySelector("textarea[aria-label=\\"스마트 워크플로 입력\\"]")?.value.includes("s03-response")', "response selection was not applied");
  progress("comparison"); await click(".api-feature-transforms", "비교의 이전 입력으로");
  await wait('!!document.querySelector("textarea[aria-label=\\"이전 버전 입력\\"]")', "comparison did not open");
  const changed = JSON.stringify({ ...JSON.parse(masked), value: 3 });
  await input('textarea[aria-label="새 버전 입력"]', changed);
  await wait('!!document.querySelector(".diff-add")', "native comparison did not produce differences");
  progress("mock-draft"); await click('[aria-label="새 버전 차이 결과"]', "Mock 초안 만들기");
  await wait('!!document.querySelector(".api-feature-webhooks:not([hidden]) .mock-draft-dialog")', "comparison result Mock preview did not open");
  await click(".mock-draft-dialog", "현재 규칙 초안 대신 적용");
  await wait(`document.querySelector("#rule-body")?.value === ${JSON.stringify(changed)}`, "Mock editor did not receive the comparison input");
  assert.equal((await success("api-studio.webhooks", "list_rules")).length, 0); assert.equal((await success("api-studio.webhooks", "server_status")).running, false);
  progress("knowledge-draft"); await click("nav", "변환");
  await wait('!!document.querySelector(".api-feature-transforms:not([hidden]) .diff-view")', "comparison state was lost on navigation");
  await click('[aria-label="새 버전 차이 결과"]', "Knowledge 초안 보관"); await click(".studio-knowledge-dialog", "마스킹 사본 보관");
  await wait('!document.querySelector(".studio-knowledge-dialog") && document.querySelector(".api-feature-transforms").textContent.includes("Knowledge 수신 연결을 사용할 수 없어")', "Knowledge fallback was not accurately reported");
  const drafts = await success("api-studio.transforms", "list_knowledge_drafts"); assert.equal(drafts.length, 1);
  const draftId = drafts[0].artifact.id; const draft = await success("api-studio.transforms", "get_knowledge_draft", { id: draftId });
  assert.ok(draft.body.includes("s03-response")); assert.ok(!draft.body.includes(secret));
  assert.equal(hits, 1); const shot = await ui.cdp.send("Page.captureScreenshot", { format: "png" }); writeFileSync("product-foundation-evidence/api-workflow-knowledge.png", Buffer.from(shot.data, "base64"));
  await stop(); progress("restart"); await start();
  assert.equal((await success("api-studio.webhooks", "server_status")).running, false); assert.equal(hits, 1);
  const reopened = await success("api-studio.transforms", "get_knowledge_draft", { id: draftId }); assert.equal(reopened.body, draft.body);
  assert.equal((await call("api-studio.api", "get_knowledge_draft", { id: draftId })).operation.outcome.state, "failed");
  await stop();
  Object.assign(evidence, { result: "pass", captureMasked: true, explicitApply: true, nativeDpapiReconnect: true, oneExplicitSend: true, responseMasked: true, nativeComparison: true, mockEditorOnly: true, listenerStopped: true, knowledgeUnavailablePreserved: true, restartPreservesDraft: true, ownerIsolation: true }); progress("complete");
} catch (error) { evidence.error = error.message; throw error; }
finally {
  ui?.cdp.close(); for (const child of live) if (child.exitCode === null && child.signalCode === null) child.kill();
  for (const policy of policies) restoreElevatedCdpPolicy(policy);
  server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  writeFileSync("product-foundation-evidence/api-workflow.json", JSON.stringify(evidence, null, 2));
}
