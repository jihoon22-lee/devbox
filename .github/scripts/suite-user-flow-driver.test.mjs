import assert from "node:assert/strict";
import test from "node:test";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
const control = { ignored: false, role: { value: "button" }, name: { value: "Continue" }, backendDOMNodeId: 12 };
function transport(nodes = [control]) {
  const calls = [];
  return {
    calls,
    async command(method, params) {
      calls.push({ method, params });
      if (method === "Accessibility.getFullAXTree") return { nodes };
      if (method === "DOM.getBoxModel") return { model: { border: [0, 0, 100, 0, 100, 30, 0, 30] } };
      return {};
    },
  };
}
test("click sends real pointer input and never evaluates a mutation", async () => {
  const cdp = transport();
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.click({ role: "button", name: "Continue" });
  assert.deepEqual(
    cdp.calls.filter((c) => c.method === "Input.dispatchMouseEvent").map((c) => c.params.type),
    ["mousePressed", "mouseReleased"],
  );
  assert.equal(
    cdp.calls.some((c) => c.method === "Runtime.evaluate"),
    false,
  );
});
test("ambiguous and disabled controls fail without clicking", async () => {
  for (const nodes of [
    [control, control],
    [{ ...control, properties: [{ name: "disabled", value: { value: true } }] }],
  ]) {
    const cdp = transport(nodes);
    const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
    await assert.rejects(ui.click({ role: "button", name: "Continue" }));
    assert.equal(
      cdp.calls.some((c) => c.method.startsWith("Input.")),
      false,
    );
  }
});
test("fill uses keyboard selection and insertion, owned close uses supplied native boundary", async () => {
  const cdp = transport();
  let closed = 0;
  const ui = createUiDriver({
    cdp,
    evidenceRoot: "/tmp/unused",
    closeOwnedWindow: async () => {
      closed++;
    },
  });
  await ui.fill({ role: "button", name: "Continue" }, "새 내용");
  await ui.closeOwnedWindow();
  assert.equal(cdp.calls.find((c) => c.method === "Input.insertText").params.text, "새 내용");
  assert.equal(closed, 1);
});
test("scope resolves only descendants of a unique accessible ancestor", async () => {
  const cdp = transport([
    {
      nodeId: "scope",
      ignored: false,
      role: { value: "region" },
      name: { value: "Owned project" },
      childIds: ["wrapper"],
    },
    { nodeId: "wrapper", ignored: true, childIds: ["owned"] },
    { ...control, nodeId: "owned" },
    { ...control, nodeId: "other", backendDOMNodeId: 13 },
  ]);
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.click({ role: "button", name: "Continue", scope: { role: "region", name: "Owned project" } });
  assert.equal(cdp.calls.find((call) => call.method === "DOM.getBoxModel").params.backendNodeId, 12);
  await assert.rejects(ui.click({ role: "button", name: "Continue", scope: { role: "region", name: "Missing" } }));
});
test("anonymous alert text observes descendants without evaluating renderer state", async () => {
  const cdp = transport([
    { nodeId: "alert", ignored: false, role: { value: "alert" }, name: { value: "" }, childIds: ["message"] },
    {
      nodeId: "message",
      ignored: false,
      role: { value: "StaticText" },
      name: { value: "Source 초안 내용을 정리해 주세요." },
    },
  ]);
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  assert.equal(await ui.text({ role: "alert", name: "" }), "Source 초안 내용을 정리해 주세요.");
  assert.equal(
    cdp.calls.some((call) => call.method === "Runtime.evaluate"),
    false,
  );
});
test("native browser confirmation uses CDP modal acceptance and rejects implicit choices", async () => {
  const cdp = transport();
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.confirmDialog(false);
  assert.deepEqual(cdp.calls.at(-1), { method: "Page.handleJavaScriptDialog", params: { accept: false } });
  await assert.rejects(ui.confirmDialog("yes"));
});
