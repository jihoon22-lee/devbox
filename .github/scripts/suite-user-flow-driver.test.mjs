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
