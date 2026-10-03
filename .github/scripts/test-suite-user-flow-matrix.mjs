import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import test from "node:test";
const matrix = JSON.parse(readFileSync(new URL("./suite-user-flow-matrix.json", import.meta.url)));
test("planned matrix has unique owned outcomes and valid local module paths", () => {
  assert.ok(matrix.length > 0);
  assert.equal(new Set(matrix.map((x) => x.id)).size, matrix.length);
  for (const row of matrix) {
    assert.match(row.id, /^[A-Z]+-\d+$/);
    assert.match(row.ownerWorkItem, /^R\d{2}$/);
    assert.match(row.module, /^windows-[a-z0-9-]+\.mjs$/);
    assert.ok(existsSync(new URL(row.module, import.meta.url)), `Missing implementation: ${row.id}/${row.module}`);
    assert.equal(row.evidenceKind, "packaged-ui");
    assert.ok(row.products.length);
  }
  const products = new Set(matrix.flatMap((row) => row.products));
  assert.deepEqual([...products].sort(), ["api-studio", "control-center", "knowledge", "workspace"]);
});
