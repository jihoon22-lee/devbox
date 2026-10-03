import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { createServer } from "node:http";
import { once } from "node:events";
import path from "node:path";

export function requireApiContext(context) {
  assert.ok(context && path.resolve(context.fixtureRoot)===path.resolve(context.root),"Prepared installed fixture required");
  assert.ok(path.basename(path.dirname(context.root)).startsWith("devbox-suite-delivery-"),"Disposable fixture root required");
  assert.match(context.installationKey ?? "",/^[a-f0-9]{64}$/);
  assert.equal(path.basename(context.namespace),`com.devbox.v08.apistudio.i${context.installationKey}`);
  assert.match(context.sourceSha ?? "",/^[a-f0-9]{40}$/);
  assert.equal(context.fixtureSha,context.sourceSha);
  assert.ok(Object.keys(context.artifactDigests ?? {}).length===7 && Object.values(context.artifactDigests).every(value=>/^[a-f0-9]{64}$/.test(value)));
  for(const method of ["click","fill","press","screenshot","confirmDialog","closeOwnedWindow"]) assert.equal(typeof context.ui?.[method],"function");
  assert.ok(context.cdp && typeof context.cdp.evaluate==="function");
  assert.equal(typeof context.nativeCall,"function");
}

export const button = (name, scope) => ({role:"button",name,...(scope ? {scope} : {})});
export const textbox = name => ({role:"textbox",name});
export async function bodyText(context) {
  return context.cdp.evaluate("document.body.innerText");
}
export async function until(check, message, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(100);
  }
  throw new Error(message);
}
export async function expectText(context, text) {
  await until(async () => (await bodyText(context)).includes(text), `Missing visible text: ${text}`);
}
export async function select(context, name, index) {
  await context.ui.click({role:"combobox", name});
  await context.ui.press("Home");
  for(let i=0;i<index;i++) await context.ui.press("ArrowDown");
  await context.ui.press("Enter");
}
export async function navigate(context, name) {
  await context.ui.click(button(name));
}
export async function scenario(context, id, actions) {
  const assertions = [];
  const screenshotPaths = [];
  try {
    await actions(message => assertions.push(message));
    assert.ok(assertions.length, "No user-flow assertions");
    screenshotPaths.push(await context.ui.screenshot(id));
    return {...contextIdentity(context),id,status:"PASS",assertions,screenshotPaths,failureCode:null};
  } catch(error) {
    try { screenshotPaths.push(await context.ui.screenshot(`${id}-failure`)); } catch {}
    return {...contextIdentity(context),id,status:"FAIL",assertions,screenshotPaths,failureCode:`${id}-assertion-failed`};
  }
}
function contextIdentity(context) {
  return {sourceSha:context.sourceSha,fixtureSha:context.fixtureSha,artifactDigests:context.artifactDigests,evidenceKind:"packaged-ui"};
}
export async function echoFixture() {
  const hits = [];
  const server = createServer(async (request,response) => {
    const chunks=[];
    for await (const chunk of request) chunks.push(chunk);
    const hit={url:request.url,method:request.method,headers:request.headers,body:Buffer.concat(chunks).toString("utf8")};
    hits.push(hit);
    const reply=()=>{response.setHeader("Content-Type","text/plain; charset=utf-8");response.end(request.url.startsWith("/delayed") ? "HTTP B late response" : `HTTP A response ${hits.length}`);};
    if(request.url.startsWith("/delayed")) setTimeout(reply,1500); else reply();
  });
  server.listen(0,"127.0.0.1");
  await once(server,"listening");
  return {url:`http://127.0.0.1:${server.address().port}`,hits,close:async()=>{
    server.closeAllConnections();
    await new Promise(resolve=>server.close(resolve));
  }};
}
