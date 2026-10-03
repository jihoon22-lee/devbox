import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import {
  establishResponseSelection,
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
