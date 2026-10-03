import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import {
  establishResponseSelection,
  diagnosticStylesConfig,
  applyDiagnosticStyles,
  prepareDiagnosticCss,
  prepareResponseSelection,
  responseTransformObservation,
  responseGeometryObservation,
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
      if (expression === responseGeometryObservation) {
        events.push("read-hit-point");
        return { body: {}, pointer: { x: 40, y: 60, intersectsViewport: true, hitsBody: true } };
      }
      events.push("scroll-body");
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
  assert.deepEqual(events, ["scroll-body", "read-hit-point", "mousePressed", "mouseReleased", "range"]);
});
test("rejects invalid pointer geometry before attempting a selection", async () => {
  await assert.rejects(
    prepareResponseSelection({
      evaluate: async () => ({ body: {}, pointer: { x: NaN, y: 20, intersectsViewport: true, hitsBody: true } }),
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

test("records failed pointer geometry before throwing a specific non-hittable error", async () => {
  const geometry = { body: {}, pointer: { x: 30, y: 1200, intersectsViewport: false, hitsBody: false } };
  let observed;
  await assert.rejects(
    prepareResponseSelection(
      { evaluate: async () => geometry, send: async () => assert.fail("must not click") },
      (value) => {
        observed = value;
      },
    ),
    /response-selection-body-not-hittable/,
  );
  assert.equal(observed, geometry);
});
test("geometry diagnostics contain layout only and distinguish clipped body from visible text", () => {
  const secret = "synthetic-private-renderer-text";
  const body = {
    tagName: "PRE",
    classList: ["resp-body"],
    textContent: secret,
    clientHeight: 28,
    scrollHeight: 28,
    parentElement: null,
    contains: () => true,
    getBoundingClientRect: () => ({
      x: 468,
      y: 1133,
      left: 468,
      top: 1133,
      right: 981,
      bottom: 1161,
      width: 513,
      height: 28,
    }),
  };
  const geometry = vm.runInNewContext(responseGeometryObservation, {
    document: { querySelector: () => body, elementFromPoint: () => assert.fail("offscreen hit must not be attempted") },
    innerWidth: 1100,
    innerHeight: 700,
    getComputedStyle: () => ({
      overflowX: "auto",
      overflowY: "auto",
      lineHeight: "28px",
      display: "block",
      visibility: "visible",
    }),
  });
  assert.equal(geometry.pointer.intersectsViewport, false);
  assert.equal(geometry.pointer.hitsBody, false);
  assert.equal(geometry.body.rect.y, 1133);
  assert.equal(geometry.viewportHeight, 700);
  assert.equal(JSON.stringify(geometry).includes(secret), false);
});

test("diagnostic styles are disabled by default and require explicit hosted source/run guards", () => {
  assert.equal(diagnosticStylesConfig({}), null);
  const env = {
    DEVBOX_API_DIAGNOSTIC_STYLES: "1",
    GITHUB_ACTIONS: "true",
    RUNNER_ENVIRONMENT: "github-hosted",
    GITHUB_EVENT_NAME: "workflow_dispatch",
    GITHUB_WORKFLOW: "Product foundation acceptance",
    GITHUB_SHA: "a".repeat(40),
    DEVBOX_SUITE_ARTIFACT_SOURCE: "b".repeat(40),
    DEVBOX_SUITE_ARTIFACT_RUN: "37126573659",
  };
  assert.equal(diagnosticStylesConfig(env).styleSourceSha, env.GITHUB_SHA);
  for (const flag of ["", "0", "true"])
    assert.throws(() => diagnosticStylesConfig({ ...env, DEVBOX_API_DIAGNOSTIC_STYLES: flag }), /invalid-flag/);
  assert.throws(() => diagnosticStylesConfig({ ...env, RUNNER_ENVIRONMENT: "self-hosted" }), /hosted-runner-required/);
  assert.throws(
    () => diagnosticStylesConfig({ ...env, GITHUB_WORKFLOW: "Windows package candidate" }),
    /diagnostic-workflow-required/,
  );
  for (const key of ["GITHUB_SHA", "DEVBOX_SUITE_ARTIFACT_SOURCE", "DEVBOX_SUITE_ARTIFACT_RUN"])
    assert.throws(() => diagnosticStylesConfig({ ...env, [key]: "" }), /valid-source-run-pair-required/);
});
test("diagnostic style injection uses inspector CSS and verifies computed layout", async () => {
  const css = ".response { min-height: 320px; }";
  const calls = [];
  const computed = {
    responsePresent: true,
    responseFlexShrink: "0",
    responseMinHeight: "320px",
    bodyPresent: true,
    bodyMinHeight: "160px",
  };
  const cdp = {
    send: async (method, params) => {
      calls.push({ method, params });
      if (method === "Page.getFrameTree") return { frameTree: { frame: { id: "root-frame" } } };
      if (method === "CSS.createStyleSheet") return { styleSheetId: "inspector-sheet" };
      return {};
    },
    evaluate: async () => computed,
  };
  assert.equal(await applyDiagnosticStyles(cdp, css), computed);
  assert.deepEqual(
    calls.map((call) => call.method),
    ["DOM.enable", "CSS.enable", "Page.getFrameTree", "CSS.createStyleSheet", "CSS.setStyleSheetText"],
  );
  assert.deepEqual(calls[3].params, { frameId: "root-frame" });
  assert.deepEqual(calls[4].params, { styleSheetId: "inspector-sheet", text: css });
  let observed;
  await assert.rejects(
    applyDiagnosticStyles(
      { ...cdp, evaluate: async () => ({ ...computed, responseMinHeight: "0px" }) },
      css,
      (value) => {
        observed = value;
      },
    ),
    /computed-style-not-confirmed/,
  );
  assert.equal(observed.responseMinHeight, "0px");
  await assert.rejects(
    applyDiagnosticStyles({ ...cdp, evaluate: async () => ({ ...computed, bodyMinHeight: "0px" }) }, css),
    /computed-style-not-confirmed/,
  );
  await assert.rejects(
    applyDiagnosticStyles(
      {
        ...cdp,
        send: async () => {
          throw new Error("CDP unavailable");
        },
      },
      css,
    ),
    /inspector-injection-failed/,
  );
});
test("diagnostic stylesheet strips exactly the two packaged imports and preserves remaining CSS", () => {
  const imports = '@import "@devbox/tokens/tokens.css";\n@import "@devbox/a11y/styles.css";\n';
  const css = "\n.response { min-height: 320px; }\n";
  assert.equal(prepareDiagnosticCss(imports + css), css);
  assert.throws(() => prepareDiagnosticCss('@import "other.css";\n' + css), /unexpected-imports/);
});
