import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { runCheck } from "./check-frontend-bundles.mjs";

const root = path.resolve(process.argv[2]);
const output = path.resolve(process.argv[3]);
const baseline = JSON.parse(readFileSync(new URL("./product-foundation-performance.json", import.meta.url), "utf8"));
const commit = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
assert.equal(commit.status, 0);
assert.equal(commit.stdout.trim(), baseline.baselineCommit);
const reports = runCheck({ scope: "apps", frontendApps: baseline.apps.join(" "), root, config: path.join(root, ".github/scripts/frontend-bundle-budgets.json") });
// The existing checker uses BigInt for exact byte arithmetic. Preserve exact
// decimal values in the evidence instead of rounding or failing JSON output.
writeFileSync(output, `${JSON.stringify({ schemaVersion: 1, source: baseline.baselineCommit, baselineTag: baseline.baselineTag, measurement: "initial frontend module closure from a source build at the pinned release commit; byte counts are decimal strings", reports }, (_key, value) => typeof value === "bigint" ? value.toString() : value, 2)}\n`, { flag: "wx" });
