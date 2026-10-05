import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { needsLegacyWarmStart } from "./windows-package-cache.mjs";

const scope = {
  repository: "jihoon22-lee/devbox",
  shard: "01",
  ref: "refs/heads/main",
  token: "fixture-token",
};
const key = "v0-rust-devbox-windows-package-shard-01-Windows_NT-x64-env-lock";
const response = (value) => async (url, options) => {
  assert.equal(url.searchParams.get("ref"), scope.ref);
  assert.equal(url.searchParams.get("per_page"), "1");
  assert.equal(url.searchParams.get("key"), key.split("Windows")[0]);
  assert.equal(options.headers.Authorization, "Bearer fixture-token");
  return { ok: true, json: async () => value };
};

test("missing namespace preserves legacy warm start", async () => {
  assert.equal(
    await needsLegacyWarmStart({ ...scope, fetchCache: response({ total_count: 0, actions_caches: [] }) }),
    true,
  );
});
test("existing current-ref shard namespace skips legacy restore", async () => {
  assert.equal(
    await needsLegacyWarmStart({
      ...scope,
      fetchCache: response({ total_count: 3, actions_caches: [{ key, ref: scope.ref }] }),
    }),
    false,
  );
});
test("invalid or foreign cache metadata preserves warm start", async () => {
  for (const value of [
    null,
    {},
    { total_count: 1, actions_caches: [] },
    { total_count: -1, actions_caches: [] },
    { total_count: 1, actions_caches: [{ key, ref: "refs/heads/other" }] },
    { total_count: 1, actions_caches: [{ key: key.replace("shard-01", "shard-02"), ref: scope.ref }] },
  ]) {
    assert.equal(await needsLegacyWarmStart({ ...scope, fetchCache: response(value) }), true);
  }
});
test("HTTP denial, invalid JSON and network failure preserve warm start", async () => {
  for (const fetchCache of [
    async () => ({ ok: false }),
    async () => ({
      ok: true,
      json: async () => {
        throw new Error("invalid JSON");
      },
    }),
    async () => {
      throw new Error("network");
    },
  ]) {
    assert.equal(await needsLegacyWarmStart({ ...scope, fetchCache }), true);
  }
});
test("invalid candidate scope is rejected before querying", async () => {
  for (const invalid of [{ shard: "../01" }, { shard: "03", ref: "refs/heads/other" }]) {
    await assert.rejects(
      needsLegacyWarmStart({ ...scope, ...invalid, fetchCache: () => assert.fail("must not query") }),
      /Invalid candidate cache scope/,
    );
  }
});
test("workflow restores legacy without saving, then saves independent shards", () => {
  const workflow = readFileSync(new URL("../workflows/windows-package-candidate.yml", import.meta.url), "utf8");
  assert.match(
    workflow,
    /if: steps\.cache-namespace\.outputs\.legacy-warm-start == 'true'[\s\S]*?shared-key: devbox-windows-package\n\s+save-if: false/,
  );
  assert.match(workflow, /shared-key: devbox-windows-package-shard-\$\{\{ matrix\.shard \}\}/);
  assert.match(workflow, /permissions:\n\s+contents: read\n\s+actions: read/);
});

test("new Center shard restores existing main shard01 cache read-only before its own namespace", () => {
  const workflow = readFileSync(new URL("../workflows/windows-package-candidate.yml", import.meta.url), "utf8");
  assert.match(
    workflow,
    /if: steps\.cache-namespace\.outputs\.legacy-warm-start == 'true' && matrix\.shard != '03'[\s\S]*?shared-key: devbox-windows-package\n\s+save-if: false/,
  );
  assert.match(
    workflow,
    /if: steps\.cache-namespace\.outputs\.legacy-warm-start == 'true' && matrix\.shard == '03'[\s\S]*?shared-key: devbox-windows-package-shard-01\n\s+save-if: false/,
  );
  assert.ok(
    workflow.indexOf("shared-key: devbox-windows-package-shard-01") <
      workflow.indexOf("shared-key: devbox-windows-package-shard-${{ matrix.shard }}"),
  );
});
