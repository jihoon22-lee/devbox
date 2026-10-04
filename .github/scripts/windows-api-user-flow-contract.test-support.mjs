import assert from "node:assert/strict";
import { test } from "node:test";
import { readFile } from "node:fs/promises";
import { scenario } from "./windows-api-user-flow-actions.mjs";
export function scenarioModuleContract(runner, ids, module) {
  test(`${ids.join(", ")} registrations match the release matrix`, async () => {
    assert.deepEqual(runner.SCENARIO_IDS, ids);
    const matrix = JSON.parse(await readFile(new URL("./suite-user-flow-matrix.json", import.meta.url), "utf8"));
    for (const id of ids)
      assert.equal(
        matrix.filter((item) => item.id === id && item.evidenceKind === "packaged-ui" && item.module === module).length,
        1,
      );
  });
  test(`${ids.join(", ")} reject missing owned installation before actions`, async () => {
    let actions = 0;
    await assert.rejects(() => runner.run({ ui: { click: () => actions++ }, fixtureRoot: "/user-data" }));
    assert.equal(actions, 0);
  });
  test(`${ids[0]} assertion failure cannot become PASS`, async () => {
    const context = {
      sourceSha: "a".repeat(40),
      fixtureSha: "a".repeat(40),
      artifactDigests: { fixture: "b".repeat(64) },
      ui: { screenshot: async () => "/owned/failure.png" },
    };
    const result = await scenario(context, ids[0], async (record) => {
      record("synthetic first observation");
      throw new Error("synthetic failure");
    });
    assert.equal(result.status, "FAIL");
    assert.equal(result.failureCode, `${ids[0]}-assertion-failed`);
    assert.deepEqual(result.assertions, ["synthetic first observation"]);
    assert.deepEqual(result.screenshotPaths, ["/owned/failure.png"]);
    assert.equal(result.evidenceKind, "packaged-ui");
    assert.equal(result.error.name, "Error");
    assert.equal(result.error.message, "synthetic failure");
  });
}
