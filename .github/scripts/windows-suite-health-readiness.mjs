import { typedComponentBridge } from "./typed-component-fixture.mjs";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const catalog = createRequire(import.meta.url)("../../apps/products.json");

// Read only the existing connection: never approve or restart a product here.
export async function ownedConnectionStatus(context) {
  return context.cdp.evaluate(`(async()=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;
    const d=await invoke('plugin:product-shell|describe');
    const requestId=crypto.randomUUID();
    const r=await invoke('plugin:suite|connection',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId,deadlineMs:Date.now()+10000,route:d.product.defaultRoute,context:d.context},method:{kind:'status'}}});
    if(r.operation.provenance.requestId!==requestId||r.operation.provenance.product!==d.product.id||r.operation.provenance.component!==d.product.id+'.commands'||r.operation.provenance.revision!==${catalog.catalogRevision}||r.operation.outcome.state!=='succeeded') throw new Error('Owned connection status unavailable');
    return {product:d.product.id,connected:r.value.connected===true,connectionState:['preparing','connected','off','failed'].includes(r.value.connectionState)?r.value.connectionState:null,generation:typeof r.value.generation==='string'&&/^[a-f0-9]{64}$/.test(r.value.generation)?r.value.generation:null,issue:typeof r.value.issue==='string'&&/^[a-z][a-z0-9_]{0,100}$/.test(r.value.issue)?r.value.issue:null};
  })()`);
}

export async function waitForOwnedHealthConnections(contexts, observe, read = ownedConnectionStatus) {
  assert.equal(contexts.length, 4, "Four owned product contexts required");
  const last = [];
  await observe(async () => {
    last.splice(0, last.length, ...(await Promise.all(contexts.map(read))));
    assert.deepEqual(
      last.map((status) => status.product).sort(),
      catalog.products.map((product) => product.id).sort(),
      "Four distinct owned products required",
    );
    for (const status of last) {
      assert.ok(["preparing", "connected", "off", "failed"].includes(status.connectionState));
      assert.ok(
        !["off", "failed"].includes(status.connectionState),
        `Owned health connection ${status.connectionState}: ${status.issue ?? "unknown"}`,
      );
    }
    if (last.some((status) => status.connectionState === "preparing")) return false;
    assert.ok(last.every((status) => status.connected === true && /^[a-f0-9]{64}$/.test(status.generation)));
    assert.equal(new Set(last.map((status) => status.generation)).size, 1, "Owned health generation differs");
    return true;
  }, "four owned native connections ready");
  return last;
}

const healthStates = new Set([
  "응답 또는 저장소를 확인하지 못함",
  "응답·저장소 선택 확인됨",
  "저장소 작업 진행 중",
  "저장소 준비 확인 필요",
  "저장소 선택 필요",
]);
export function projectHealthRows(rows) {
  return rows.slice(0, 4).map((cells) => ({
    product: catalog.products.find((product) => product.label === cells[0])?.id ?? null,
    version: typeof cells[1] === "string" && /^\d+\.\d+\.\d+$/.test(cells[1]) ? cells[1] : null,
    status: healthStates.has(cells[2]) ? cells[2] : null,
  }));
}

const startupIssues = new Set([
  "import_schema_unsupported",
  "import_database_invalid",
  "import_restart_required",
  "unavailable",
  "store_future_schema",
  "store_manifest_invalid",
  "future_schema",
  "journal_unavailable",
  "setup_required",
  "store_unavailable",
  "store_invalid",
  "store_busy",
  "store_path_invalid",
  "vault_binding_unavailable",
  "vault_binding_invalid",
  "vault_owner_busy",
  "vault_owner_unavailable",
  "vault_change_conflict",
  "component_initialization_failed",
  "component_state_conflict",
]);
export function projectKnowledgeStartupStatus(result) {
  if (result?.admissionRejected === true)
    return {
      observationUnavailable: true,
      admissionCode: [
        "unauthorized",
        "invalid-request",
        "expired",
        "replayed",
        "overloaded",
        "stale-context",
        "unavailable",
      ].includes(result.code)
        ? result.code
        : null,
      deliveryState: ["direct", "import", "health", "committed", "recover"].includes(result.deliveryState)
        ? result.deliveryState
        : null,
    };
  const issue = result?.value?.issue ?? result?.operation?.outcome?.code;
  return {
    state: ["succeeded", "failed", "cancelled", "expired"].includes(result?.operation?.outcome?.state)
      ? result.operation.outcome.state
      : null,
    code: startupIssues.has(issue) ? issue : null,
    active:
      result?.operation?.outcome?.state === "succeeded" && typeof result?.value?.active === "boolean"
        ? result.value.active
        : null,
    prepared:
      result?.operation?.outcome?.state === "succeeded" && typeof result?.value?.prepared === "boolean"
        ? result.value.prepared
        : null,
    hasExisting:
      result?.operation?.outcome?.state === "succeeded" && typeof result?.value?.hasExisting === "boolean"
        ? result.value.hasExisting
        : null,
    bindingUnavailable:
      result?.operation?.outcome?.state === "succeeded" && typeof result?.value?.bindingUnavailable === "boolean"
        ? result.value.bindingUnavailable
        : null,
    vaultChange:
      result?.operation?.outcome?.state === "succeeded" && typeof result?.value?.vaultChange === "boolean"
        ? result.value.vaultChange
        : null,
  };
}
export async function ownedKnowledgeStartupStatus(context) {
  assert.ok(typeof context.executable === "string" && /(?:^|[\\/])devbox-knowledge\.exe$/i.test(context.executable));
  assert.ok(typeof context.processIdentity?.Path === "string");
  assert.equal(
    context.processIdentity?.Path?.toLowerCase(),
    context.executable?.toLowerCase(),
    "Owned Knowledge image required",
  );
  const result = await context.cdp.evaluate(`(async()=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;${typedComponentBridge}
    const d=await invoke('plugin:product-shell|describe');
    if(d.product.id!=='knowledge')throw new Error('Owned Knowledge status unavailable');
    const requestId=crypto.randomUUID();
    let r;
    try { r=await invokeComponent('knowledge',{request:{header:{protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId,deadlineMs:Date.now()+5000,route:'notes',context:d.context},component:'knowledge.setup',method:'status',args:{}}}); }
    catch(error) {
      const p=error?.provenance;
      if(p?.requestId!==requestId||p.product!=='knowledge'||p.component!=='knowledge.setup'||p.revision!==${catalog.catalogRevision})throw new Error('Owned Knowledge admission unavailable');
      return {admissionRejected:true,code:error.code,deliveryState:d.deliveryState};
    }
    if(r.operation.provenance.requestId!==requestId||r.operation.provenance.product!=='knowledge'||r.operation.provenance.component!=='knowledge.setup'||r.operation.provenance.revision!==${catalog.catalogRevision})throw new Error('Owned Knowledge status unavailable');
    return r;
  })()`);
  return projectKnowledgeStartupStatus(result);
}
