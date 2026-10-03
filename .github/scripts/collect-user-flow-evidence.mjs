import assert from "node:assert/strict";
import { lstat, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { packagedIdentity, fileDigest } from "./suite-user-flow-results.mjs";
import { requireCompleteEvidence } from "./suite-user-flow-evidence.mjs";
async function reports(root) {
  const found = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const file = path.join(root, entry.name);
    assert.ok(!entry.isSymbolicLink(), "Evidence links forbidden");
    if (entry.isDirectory()) found.push(...(await reports(file)));
    else if (path.basename(root) === "user-flows" && entry.name.endsWith(".json")) found.push(file);
  }
  return found;
}
export async function collectUserFlowEvidence({ assets, sourceSha, input, output }) {
  const identity = await packagedIdentity(assets, sourceSha);
  const matrix = JSON.parse(await readFile(new URL("./suite-user-flow-matrix.json", import.meta.url), "utf8"));
  const results = [],
    screenshots = {};
  for (const report of await reports(input)) {
    const document = JSON.parse(await readFile(report, "utf8"));
    assert.equal(document.schemaVersion, 1);
    assert.ok(Array.isArray(document.results));
    for (const result of document.results) {
      const paths = [];
      for (const item of result.screenshotPaths ?? []) {
        assert.ok(
          typeof item === "string" && !item.includes("\\") && !item.split("/").includes("..") && !path.isAbsolute(item),
        );
        const file = path.resolve(path.dirname(report), item);
        const info = await lstat(file);
        assert.ok(info.isFile() && !info.isSymbolicLink() && info.size > 8, "Missing screenshot evidence");
        const bytes = await readFile(file);
        assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", "PNG evidence required");
        const key = path.relative(input, file).split(path.sep).join("/");
        screenshots[key] = await fileDigest(file);
        paths.push(key);
      }
      results.push({ ...result, screenshotPaths: paths });
    }
  }
  const installationKey = results.find((row) => row.id === "INSTALL-01")?.installationKey;
  assert.match(installationKey ?? "", /^[a-f0-9]{64}$/);
  assert.ok(
    results.every(
      (row) =>
        row.installationKey === installationKey ||
        (row.id === "DELIVERY-01" &&
          row.fixtureKind === "legacy-upgrade" &&
          row.parentInstallationKey === installationKey &&
          /^[a-f0-9]{64}$/.test(row.installationKey)),
    ),
    "Current-product journeys must follow the interactive installation; the pinned legacy upgrade owns a separate child fixture",
  );
  const expected = {
    expectedSource: identity.sourceSha,
    expectedFixture: identity.fixtureSha,
    expectedDigests: identity.artifactDigests,
  };
  const summary = requireCompleteEvidence({ requiredMatrix: matrix, results, ...expected });
  const evidence = { schemaVersion: 1, installationKey, ...expected, results, screenshots, summary };
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(evidence, null, 2)}\n`, { flag: "wx" });
  return evidence;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [assets, sourceSha, input, output] = process.argv.slice(2);
  const evidence = await collectUserFlowEvidence({ assets, sourceSha, input, output });
  console.log(JSON.stringify(evidence.summary));
}
