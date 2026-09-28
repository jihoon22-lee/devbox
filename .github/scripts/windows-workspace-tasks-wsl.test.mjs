import assert from "node:assert/strict";
import { test } from "node:test";
import { reviewWslTaskDefinitions } from "./windows-workspace-tasks-wsl.mjs";
test("WSL task fixture reviews project definitions independently of Runtime source trust", async () => {
  const calls = [];
  let trusted = false;
  await reviewWslTaskDefinitions(async (method, args = {}) => {
    calls.push({ method, args });
    if (method === "load") return { definitionsTrusted: trusted };
    if (method === "preview_trust") return { previewId: "native-review" };
    assert.equal(method, "approve_trust");
    assert.deepEqual(args, { previewId: "native-review" });
    trusted = true;
  });
  assert.deepEqual(
    calls.map((call) => call.method),
    ["load", "preview_trust", "approve_trust", "load"],
  );
});
test("a failed definition approval is never treated as ready to execute", async () => {
  await assert.rejects(
    reviewWslTaskDefinitions(async (method) => {
      if (method === "load") return { definitionsTrusted: false };
      if (method === "preview_trust") return { previewId: "stale" };
      throw new Error("stale review");
    }),
    /stale review/,
  );
});
