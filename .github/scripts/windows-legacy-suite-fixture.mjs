// Pinned v0.8.1 is fixture input only. Candidate bytes and promotion identity stay unchanged.
import assert from "node:assert/strict";
import { mkdir, writeFile, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileDigest } from "./suite-user-flow-results.mjs";
export const legacySource = "1c97b41ee10ca0df7c062338bfe85659af025a89";
export const legacySetupHash = "ff20ee2d45365bbd2d98526d56dbf0e71b0273bc70ddb4f54dd8d0725b4004b3";
const base = "https://github.com/jihoon22-lee/devbox/releases/download/v0.8.1/";
export async function prepareLegacySuite(directory) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  assert.ok(path.resolve(directory).startsWith(path.resolve(process.env.RUNNER_TEMP) + path.sep));
  await mkdir(directory);
  async function download(name, size, hash) {
    assert.match(name, /^[a-zA-Z0-9_.-]+$/);
    const response = await fetch(base + name, { signal: AbortSignal.timeout(120000) });
    assert.equal(response.status, 200);
    const content = Buffer.from(await response.arrayBuffer());
    assert.ok(content.length <= 80 * 1024 * 1024, "Bounded legacy release asset");
    if (size !== undefined) assert.equal(content.length, size);
    const file = path.join(directory, name);
    await writeFile(file, content, { flag: "wx" });
    if (hash) assert.equal(await fileDigest(file), hash, "Pinned legacy bytes changed");
    return file;
  }
  const manifestFile = await download(
    "release-manifest.json",
    4333,
    "2ea4f0c28b21943e871bed79ec86c8ab7bb875ded1ddfeb643d6f14c849c436a",
  );
  const manifest = JSON.parse(await readFile(manifestFile, "utf8"));
  assert.equal(manifest.sourceSha, legacySource);
  assert.equal(manifest.releaseTag, "v0.8.1");
  assert.equal(manifest.setup.sha256, legacySetupHash);
  assert.deepEqual(
    manifest.products.map((p) => p.id),
    ["workspace", "api-studio", "knowledge", "control-center"],
  );
  for (const entry of [manifest.setup, manifest.notices, ...manifest.products.map((p) => p.portable)])
    await download(entry.name, entry.size, entry.sha256);
  const payload = {
    schemaVersion: 1,
    suiteVersion: manifest.suiteVersion,
    sourceSha: legacySource,
    protocolVersion: 1,
    products: manifest.products,
    notices: manifest.notices,
  };
  const payloadPath = path.join(directory, "suite-payload.json");
  await writeFile(payloadPath, JSON.stringify(payload, null, 2) + "\n", { flag: "wx" });
  const center = manifest.products.find((p) => p.id === "control-center");
  const helper = path.join(directory, "devbox-suite-bootstrap.exe");
  const extraction = spawnSync(
    "python",
    [
      "-c",
      "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); b=z.read('resources/suite/devbox-suite-bootstrap.exe'); open(sys.argv[2],'xb').write(b)",
      path.join(directory, center.portable.name),
      helper,
    ],
    { encoding: "utf8", timeout: 30000 },
  );
  assert.equal(extraction.status, 0);
  const helperFile = center.files.find((f) => f.name === "resources/suite/devbox-suite-bootstrap.exe");
  assert.equal((await stat(helper)).size, helperFile.size);
  assert.equal(await fileDigest(helper), helperFile.sha256);
  return { directory, manifest, helper, payloadPath, setup: path.join(directory, manifest.setup.name) };
}
