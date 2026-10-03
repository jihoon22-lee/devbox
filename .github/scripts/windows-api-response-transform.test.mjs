import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import {
  establishResponseSelection,
  prepareResponseSelection,
  responseTransformObservation,
  waitForResponseTransform,
} from "./windows-api-response-transform.mjs";

const ready = {
  senderAcknowledged: true,
  transformsVisible: true,
  receiverPreview: true,
  senderIssue: null,
  receiverAlert: false,
};
const until = async (check) => {
  for (let attempt = 0; attempt < 3; attempt++) if (await check()) return;
  throw new Error("bounded observation timeout");
};
test("requires both sender acknowledgment and recipient preview, recording intermediate states", async () => {
  const states = [{ ...ready, senderAcknowledged: false }, { ...ready, receiverPreview: false }, ready];
  const observed = [];
  await waitForResponseTransform({ evaluate: async () => states.shift() }, until, (state) => observed.push(state));
  assert.equal(observed.length, 3);
});
test("reports sender rejection before a generic preview timeout", async () => {
  for (const senderIssue of ["selection-empty", "selection-outside", "selection-stale", "native-send-failed"]) {
    let observed;
    await assert.rejects(
      waitForResponseTransform({ evaluate: async () => ({ ...ready, senderIssue }) }, until, (state) => {
        observed = state;
      }),
      new RegExp(`response-transform-${senderIssue}`),
    );
    assert.equal(observed.senderIssue, senderIssue);
  }
});
test("reports recipient alert and never substitutes it for preview acceptance", async () => {
  await assert.rejects(
    waitForResponseTransform(
      { evaluate: async () => ({ ...ready, receiverPreview: false, receiverAlert: true }) },
      until,
      () => {},
    ),
    /receiver-alert/,
  );
});
test("establishes selection and synchronizes the snapshot with a synthetic renderer selectionchange event", () => {
  const events = [];
  const body = {};
  const selection = { removeAllRanges: () => events.push("clear"), addRange: () => events.push("select") };
  vm.runInNewContext(establishResponseSelection, {
    document: {
      querySelector: () => body,
      createRange: () => ({ selectNodeContents: (node) => assert.equal(node, body) }),
      dispatchEvent: (event) => events.push(event.type),
    },
    getSelection: () => selection,
    Event: class {
      constructor(type) {
        this.type = type;
      }
    },
  });
  assert.deepEqual(events, ["clear", "select", "selectionchange"]);
});
test("failure observations exclude arbitrary renderer feedback and selected text", () => {
  const secret = "synthetic-private-renderer-text";
  const feedback = { textContent: secret, classList: { contains: (name) => name === "error" } };
  const state = vm.runInNewContext(responseTransformObservation, {
    document: { querySelector: (selector) => (selector.includes("response-feedback") ? feedback : null) },
    getSelection: () => ({ rangeCount: 0, toString: () => secret }),
  });
  assert.equal(state.senderIssue, "sender-error");
  assert.equal(JSON.stringify(state).includes(secret), false);
});

test("uses a real pointer press and release before constructing the synthetic Range", async () => {
  const events = [];
  const cdp = {
    evaluate: async (expression) => {
      if (expression === establishResponseSelection) {
        events.push("range");
        return;
      }
      events.push("read-hit-point");
      return { x: 40, y: 60 };
    },
    send: async (method, params) => {
      assert.equal(method, "Input.dispatchMouseEvent");
      assert.equal(params.x, 40);
      assert.equal(params.y, 60);
      assert.equal(params.button, "left");
      events.push(params.type);
    },
  };
  await prepareResponseSelection(cdp);
  assert.deepEqual(events, ["read-hit-point", "mousePressed", "mouseReleased", "range"]);
});
test("rejects invalid pointer geometry before attempting a selection", async () => {
  await assert.rejects(
    prepareResponseSelection({
      evaluate: async () => ({ x: NaN, y: 20 }),
      send: async () => assert.fail("must not click"),
    }),
    /pointer-invalid/,
  );
});
test("distinguishes nonempty body and DOM Range from an empty focused-input Selection without exporting text", () => {
  const secret = "synthetic-private-renderer-text";
  const body = {
    textContent: secret,
    contains: () => true,
    closest: () => null,
    getBoundingClientRect: () => ({ x: 1, y: 2, width: 100, height: 50 }),
  };
  const range = { startContainer: {}, endContainer: {}, toString: () => secret };
  const state = vm.runInNewContext(responseTransformObservation, {
    document: {
      activeElement: { tagName: "INPUT", type: "text", value: secret },
      querySelector: (selector) => (selector.endsWith(".resp-body") ? body : null),
    },
    getSelection: () => ({ rangeCount: 1, getRangeAt: () => range, toString: () => "" }),
    getComputedStyle: () => ({ display: "block", visibility: "visible" }),
  });
  assert.equal(state.bodyTextNonempty, true);
  assert.equal(state.rangeTextNonempty, true);
  assert.equal(state.selectionInsideBody, true);
  assert.equal(state.selectionNonempty, false);
  assert.equal(state.bodyVisible, true);
  assert.equal(state.activeElementTag, "INPUT");
  assert.equal(state.activeElementType, "text");
  assert.equal(JSON.stringify(state).includes(secret), false);
});
