import assert from "node:assert/strict";
import test from "node:test";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { leaveWorkspaceEditorByKeyboard } from "./windows-workspace-input-ui.mjs";
test("font-control traversal uses CodeMirror's explicit Escape Tab exit without document mutation", async () => {
  const pressed = [];
  await leaveWorkspaceEditorByKeyboard({ press: async (key) => pressed.push(key) }, async () => true);
  assert.deepEqual(pressed, ["Escape", "Tab"]);
  await leaveWorkspaceEditorByKeyboard({ press: async () => assert.fail("Unexpected input") }, async () => false);
});
test("keyboard prompt decision runs before the pending key acknowledgement and requires explicit intent", async () => {
  const cdp = transport();
  let listener,
    release,
    unsubscribed = false;
  cdp.onEvent = (_event, callback) => {
    listener = callback;
    return () => {
      unsubscribed = true;
    };
  };
  const command = cdp.command;
  cdp.command = async (method, params) => {
    if (method === "Input.dispatchKeyEvent" && params.type === "keyDown") {
      const pending = new Promise((resolve) => {
        release = resolve;
      });
      listener({ type: "prompt" });
      return pending;
    }
    if (method === "Page.handleJavaScriptDialog") {
      assert.deepEqual(params, { accept: true, promptText: "owned-name" });
      release({});
    }
    return command(method, params);
  };
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.pressWithPrompt("Enter", undefined), /Explicit prompt/);
  await ui.pressWithPrompt("Enter", true, "owned-name");
  assert.equal(unsubscribed, true);
});
const control = { ignored: false, role: { value: "button" }, name: { value: "Continue" }, backendDOMNodeId: 12 };
function transport(
  nodes = [control],
  { hit = 12, contained = false, failContains = false, pageX = 0, pageY = 0 } = {},
) {
  const calls = [];
  return {
    calls,
    async command(method, params) {
      calls.push({ method, params });
      if (method === "Accessibility.getFullAXTree") return { nodes };
      if (method === "DOM.getNodeForLocation") return { backendNodeId: hit };
      if (method === "Page.getLayoutMetrics")
        return { cssLayoutViewport: { pageX, pageY, clientWidth: 1024, clientHeight: 720 } };
      if (method === "DOM.resolveNode") return { object: { objectId: `owned-${params.backendNodeId}` } };
      if (method === "Runtime.callFunctionOn") {
        if (failContains) throw new Error("Owned DOM detached");
        return { result: { value: contained } };
      }
      if (method === "DOM.getBoxModel") return { model: { border: [0, 0, 100, 0, 100, 30, 0, 30] } };
      return {};
    },
  };
}
test("root scrolling uses page coordinates for hit testing and viewport coordinates for real pointer input", async () => {
  const cdp = transport([control], { pageX: 125, pageY: 1242 });
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.click({ role: "button", name: "Continue" });
  const hit = cdp.calls.find((call) => call.method === "DOM.getNodeForLocation");
  assert.equal(hit.params.x, 175);
  assert.equal(hit.params.y, 1257);
  for (const event of cdp.calls.filter((call) => call.method === "Input.dispatchMouseEvent")) {
    assert.equal(event.params.x, 50);
    assert.equal(event.params.y, 15);
  }
});
test("invalid root scroll geometry fails before hit testing or pointer input", async () => {
  const cdp = transport([control], { pageY: NaN });
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.click({ role: "button", name: "Continue" }), /Page layout unavailable/);
  assert.equal(
    cdp.calls.some((call) => call.method === "DOM.getNodeForLocation" || call.method.startsWith("Input.")),
    false,
  );
});
test("offscreen control center fails before page hit testing or pointer input", async () => {
  const cdp = transport();
  const command = cdp.command;
  cdp.command = async (method, params) =>
    method === "DOM.getBoxModel"
      ? { model: { border: [0, 800, 100, 800, 100, 900, 0, 900] } }
      : command(method, params);
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.click({ role: "button", name: "Continue" }), /Control center outside viewport/);
  assert.equal(
    cdp.calls.some((call) => call.method === "DOM.getNodeForLocation" || call.method.startsWith("Input.")),
    false,
  );
});
test("a tall visible editor receives one click inside its viewport intersection", async () => {
  const cdp = transport();
  const command = cdp.command;
  cdp.command = async (method, params) =>
    method === "DOM.getBoxModel"
      ? { model: { border: [240, 323, 690, 323, 690, 1800, 240, 1800] } }
      : command(method, params);
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.click({ role: "button", name: "Continue" });
  const clicks = cdp.calls.filter((call) => call.method === "Input.dispatchMouseEvent");
  assert.equal(clicks.length, 2);
  assert.ok(clicks.every((call) => call.params.y >= 323 && call.params.y < 720));
});
test("a viewport intersection covered by a clipped ancestor never receives input", async () => {
  const cdp = transport([control], { hit: 44, contained: false });
  const command = cdp.command;
  cdp.command = async (method, params) =>
    method === "DOM.getBoxModel"
      ? { model: { border: [240, 323, 690, 323, 690, 1800, 240, 1800] } }
      : command(method, params);
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.click({ role: "button", name: "Continue" }), /covered by another element/);
  assert.equal(
    cdp.calls.some((call) => call.method.startsWith("Input.")),
    false,
  );
});
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

test("an overlay hit cannot dispatch pointer input and resolved handles are released", async () => {
  const cdp = transport([control], { hit: 13 });
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.click({ role: "button", name: "Continue" }));
  assert.equal(
    cdp.calls.some((call) => call.method.startsWith("Input.")),
    false,
  );
  assert.deepEqual(
    cdp.calls
      .filter((call) => call.method === "Runtime.releaseObject")
      .map((call) => call.params.objectId)
      .sort(),
    ["owned-12", "owned-13"],
  );
});
test("a verified descendant hit dispatches input only after read-only validation and cleanup", async () => {
  const cdp = transport([control], { hit: 13, contained: true });
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.click({ role: "button", name: "Continue" });
  const read = cdp.calls.find((call) => call.method === "Runtime.callFunctionOn");
  assert.equal(read.params.objectId, "owned-12");
  assert.deepEqual(read.params.arguments, [{ objectId: "owned-13" }]);
  assert.equal(cdp.calls.filter((call) => call.method === "Runtime.releaseObject").length, 2);
  assert.ok(
    cdp.calls.findIndex((call) => call.method === "Input.dispatchMouseEvent") >
      cdp.calls.findLastIndex((call) => call.method === "Runtime.releaseObject"),
  );
});
test("a detached DOM validation fails closed while releasing both handles", async () => {
  const cdp = transport([control], { hit: 13, failContains: true });
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.click({ role: "button", name: "Continue" }));
  assert.equal(
    cdp.calls.some((call) => call.method.startsWith("Input.")),
    false,
  );
  assert.equal(cdp.calls.filter((call) => call.method === "Runtime.releaseObject").length, 2);
});

test("target readiness observes async missing/disabled AX states without input then clicks once", async () => {
  const cdp = transport();
  const command = cdp.command;
  let reads = 0;
  cdp.command = async (method, params) => {
    if (method === "Accessibility.getFullAXTree") {
      reads++;
      cdp.calls.push({ method, params });
      return {
        nodes:
          reads === 1
            ? []
            : reads === 2
              ? [{ ...control, properties: [{ name: "disabled", value: { value: true } }] }]
              : [control],
      };
    }
    return command(method, params);
  };
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.waitForTarget({ role: "button", name: "Continue" });
  assert.equal(reads, 3);
  assert.equal(
    cdp.calls.some((call) => call.method.startsWith("Input.") || call.method === "Runtime.evaluate"),
    false,
  );
  await ui.click({ role: "button", name: "Continue" });
  assert.equal(cdp.calls.filter((call) => call.method === "Input.dispatchMouseEvent").length, 2);
});
test("target readiness rejects ambiguity immediately and times out absent targets without action", async () => {
  for (const nodes of [[control, control], []]) {
    const cdp = transport(nodes);
    const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
    await assert.rejects(
      ui.waitForTarget({ role: "button", name: "Continue" }, { timeoutMs: 1 }),
      nodes.length ? /found 2/ : /Timed out waiting/,
    );
    assert.equal(
      cdp.calls.some((call) => call.method.startsWith("Input.")),
      false,
    );
    if (nodes.length) assert.equal(cdp.calls.length, 1);
  }
});

test("native Enter and Space carry character text while shortcuts carry physical codes only", async () => {
  const cdp = transport();
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  for (const key of ["Enter", "Space", "Control+0", "Control+a", "Shift+Tab"]) await ui.press(key);
  const events = cdp.calls.filter((call) => call.method === "Input.dispatchKeyEvent").map((call) => call.params);
  assert.equal(events[0].text, "\r");
  assert.equal(events[0].code, "Enter");
  assert.equal(events[1].text, undefined);
  assert.equal(events[2].text, " ");
  assert.equal(events[2].code, "Space");
  assert.equal(events[4].code, "Digit0");
  assert.equal(events[4].text, undefined);
  assert.equal(events[6].code, "KeyA");
  assert.equal(events[6].text, undefined);
  assert.equal(events[8].code, "Tab");
  assert.equal(events[8].text, undefined);
});

test("explicit dialog decision handles opening before the mouse release acknowledgement", async () => {
  const cdp = transport();
  let opening, acknowledge;
  cdp.onEvent = (method, callback) => {
    assert.equal(method, "Page.javascriptDialogOpening");
    opening = callback;
    return () => {};
  };
  const command = cdp.command.bind(cdp);
  cdp.command = async (method, params) => {
    if (method === "Input.dispatchMouseEvent" && params.type === "mouseReleased") {
      await command(method, params);
      return new Promise((resolve) => {
        acknowledge = resolve;
        opening({ type: "confirm" });
        opening({ type: "confirm" });
      });
    }
    if (method === "Page.handleJavaScriptDialog") {
      assert.equal(params.accept, false);
      acknowledge();
    }
    return command(method, params);
  };
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await ui.clickWithDialog({ role: "button", name: "Continue" }, false);
  assert.equal(cdp.calls.filter(({ method }) => method === "Page.handleJavaScriptDialog").length, 1);
  assert.equal(
    cdp.calls.filter(({ method, params }) => method === "Input.dispatchMouseEvent" && params.type === "mouseReleased")
      .length,
    1,
  );
});

test("dialog click requires explicit Boolean intent before input", async () => {
  const cdp = transport();
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  await assert.rejects(ui.clickWithDialog({ role: "button", name: "Continue" }, undefined), /Explicit dialog/);
  assert.equal(cdp.calls.length, 0);
});

test("slow geometry does not arm a dialog timeout or leave input after failure", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const cdp = transport();
  const command = cdp.command.bind(cdp);
  let releaseGeometry,
    opening,
    listenerCount = 0,
    releasePointer;
  cdp.onEvent = (_method, callback) => {
    listenerCount++;
    opening = callback;
    return () => listenerCount--;
  };
  cdp.command = async (method, params) => {
    if (method === "Accessibility.getFullAXTree")
      await new Promise((resolve) => {
        releaseGeometry = resolve;
      });
    if (method === "Input.dispatchMouseEvent" && params.type === "mouseReleased") {
      await command(method, params);
      return new Promise((resolve) => {
        releasePointer = resolve;
        opening({ type: "alert" });
      });
    }
    return command(method, params);
  };
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  let settled = false;
  const pending = ui.clickWithDialog({ role: "button", name: "Continue" }, false).finally(() => {
    settled = true;
  });
  t.mock.timers.tick(10001);
  assert.equal(listenerCount, 0, "geometry must finish before the dialog listener/timer is armed");
  releaseGeometry();
  for (let i = 0; i < 100 && !releasePointer; i++) await Promise.resolve();
  assert.equal(settled, false, "invalid dialog cannot return while the pointer response remains in flight");
  assert.equal(listenerCount, 1);
  releasePointer();
  await assert.rejects(pending, /Expected confirmation dialog type/);
  assert.equal(listenerCount, 0);
  assert.equal(cdp.calls.filter(({ method }) => method === "Page.handleJavaScriptDialog").length, 0);
});

test("opening timeout stops once a valid dialog decision command is underway", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const cdp = transport(),
    command = cdp.command.bind(cdp);
  let opening, releasePointer, rejectDecision;
  cdp.onEvent = (_method, callback) => {
    opening = callback;
    return () => {};
  };
  cdp.command = async (method, params) => {
    if (method === "Input.dispatchMouseEvent" && params.type === "mouseReleased") {
      await command(method, params);
      return new Promise((resolve) => {
        releasePointer = resolve;
        opening({ type: "confirm" });
      });
    }
    if (method === "Page.handleJavaScriptDialog")
      return new Promise((_resolve, reject) => {
        rejectDecision = reject;
      });
    return command(method, params);
  };
  const ui = createUiDriver({ cdp, evidenceRoot: "/tmp/unused", closeOwnedWindow: async () => {} });
  const pending = ui.clickWithDialog({ role: "button", name: "Continue" }, true);
  for (let i = 0; i < 100 && !rejectDecision; i++) await Promise.resolve();
  t.mock.timers.tick(10001);
  rejectDecision(new Error("CDP decision command timeout"));
  releasePointer();
  await assert.rejects(pending, /CDP decision command timeout/);
});
