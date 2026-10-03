import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import path from "node:path";
const sha = (value) => typeof value === "string" && /^[a-f0-9]{40}$/.test(value);
const digestMap = (value) =>
  value &&
  typeof value === "object" &&
  !Array.isArray(value) &&
  Object.keys(value).length > 0 &&
  Object.values(value).every((v) => typeof v === "string" && /^[a-f0-9]{64}$/.test(v));
export function summarizeEvidence(
  matrix,
  results,
  { expectedSource, expectedFixture, expectedDigests, ownerWorkItem } = {},
) {
  const selected = ownerWorkItem ? matrix.filter((row) => row.ownerWorkItem === ownerWorkItem) : matrix;
  const missing = [],
    failures = [];
  if (!selected.length || new Set(matrix.map((row) => row.id)).size !== matrix.length) failures.push("invalid-matrix");
  if (!sha(expectedSource) || !sha(expectedFixture) || !digestMap(expectedDigests))
    failures.push("invalid-expected-identity");
  for (const row of selected) {
    const found = results.filter((result) => result.id === row.id);
    if (!found.length) {
      missing.push(row.id);
      continue;
    }
    const result = found[0];
    const sameDigests =
      digestMap(result.artifactDigests) &&
      digestMap(expectedDigests) &&
      Object.keys(result.artifactDigests).length === Object.keys(expectedDigests).length &&
      Object.entries(expectedDigests).every(([key, value]) => result.artifactDigests[key] === value);
    if (
      found.length !== 1 ||
      result.status !== "PASS" ||
      result.sourceSha !== expectedSource ||
      result.fixtureSha !== expectedFixture ||
      !sameDigests ||
      result.evidenceKind !== row.evidenceKind ||
      !Array.isArray(result.assertions) ||
      !result.assertions.length ||
      !result.assertions.every((x) => typeof x === "string" && x.length > 0) ||
      !Array.isArray(result.screenshotPaths) ||
      (row.evidenceKind === "packaged-ui" && !result.screenshotPaths.length) ||
      result.failureCode !== null
    )
      failures.push(row.id);
  }
  for (const result of results) if (!matrix.some((row) => row.id === result.id)) failures.push(`unknown:${result.id}`);
  return {
    ready: missing.length === 0 && failures.length === 0,
    scope: ownerWorkItem ? `work-item:${ownerWorkItem}` : "release",
    missing,
    failures,
  };
}
export function requireCompleteEvidence({ requiredMatrix, results, ...expected }) {
  const summary = summarizeEvidence(requiredMatrix, results, expected);
  if (!summary.ready) throw new Error(`User-flow evidence incomplete: ${JSON.stringify(summary)}`);
  return summary;
}
export async function runUserFlows(matrix, context, { ownerWorkItem } = {}) {
  const selected = ownerWorkItem ? matrix.filter((row) => row.ownerWorkItem === ownerWorkItem) : matrix;
  if (!selected.length) throw new Error("No matching user-flow work item");
  const results = [];
  for (const module of new Set(selected.map((row) => row.module))) {
    if (!/^windows-[a-z0-9-]+\.mjs$/.test(module)) throw new Error("Invalid user-flow module");
    const runner = await import(new URL(module, import.meta.url));
    const records = await runner.run(context);
    if (!Array.isArray(records)) throw new Error("Invalid user-flow result");
    results.push(...records);
  }
  return {
    results,
    ...summarizeEvidence(matrix, results, {
      expectedSource: context.sourceSha,
      expectedFixture: context.fixtureSha,
      expectedDigests: context.artifactDigests,
      ownerWorkItem,
    }),
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const ownerIndex = args.indexOf("--owner-work-item");
  if (!args.includes("--all-required") && ownerIndex < 0) throw new Error("Explicit evidence scope required");
  if (args.includes("--all-required") && ownerIndex >= 0) throw new Error("Choose one evidence scope");
  const file = args[0];
  const input = JSON.parse(await readFile(file, "utf8"));
  const matrix = JSON.parse(await readFile(new URL("./suite-user-flow-matrix.json", import.meta.url), "utf8"));
  const summary = requireCompleteEvidence({
    requiredMatrix: matrix,
    ...input,
    ownerWorkItem: ownerIndex < 0 ? undefined : args[ownerIndex + 1],
  });
  process.stdout.write(`${JSON.stringify(summary)}\n`);
}
