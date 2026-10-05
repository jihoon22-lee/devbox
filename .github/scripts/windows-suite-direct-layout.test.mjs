import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { copyDirectProductImage } from "./windows-suite-direct-layout.mjs";
import { fileDigest } from "./suite-user-flow-results.mjs";
import { productWindowForImage } from "./windows-user-flow-window.mjs";

test("renamed direct images retain product lifecycle filtering only inside their owned copy", () => {
  const root = `C:\\Temp\\devbox-suite-delivery-${"a".repeat(32)}`;
  for (const product of ["workspace", "api-studio", "knowledge", "control-center"]) {
    const name = `direct-${product}-Ab123x`;
    assert.equal(productWindowForImage(`${root}\\${name}\\${name}.exe`, root), product);
    assert.equal(productWindowForImage(`${root}\\different\\${name}.exe`, root), undefined);
    assert.equal(productWindowForImage(`C:\\foreign\\${name}\\${name}.exe`, root), undefined);
    assert.equal(productWindowForImage(`${root}\\${name}\\${name}-extra.exe`, root), undefined);
    assert.equal(productWindowForImage(`${root}\\devbox-${product}.exe`, root), product);
  }
});

test("concurrent direct copy has its own CDP policy name while preserving exact image and resources", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "direct-layout-test-"));
  try {
    const installed = path.join(directory, "installed");
    await mkdir(path.join(installed, "resources"), { recursive: true });
    const source = path.join(installed, "devbox-api-studio.exe");
    await writeFile(source, "synthetic exact candidate bytes");
    await writeFile(path.join(installed, "resources", "companion"), "owned resource");
    const occupiedNames = new Set([path.basename(source).toLowerCase()]);
    for (const suffix of ["one", "two"]) {
      const root = path.join(directory, `direct-api-studio-${suffix}`);
      const executable = await copyDirectProductImage(source, root);
      assert.equal(
        occupiedNames.has(path.basename(executable).toLowerCase()),
        false,
        "WebView2 CDP policy value already exists",
      );
      occupiedNames.add(path.basename(executable).toLowerCase());
      assert.equal(path.dirname(executable), root);
      assert.equal(await fileDigest(executable), await fileDigest(source));
      assert.equal(await readFile(path.join(root, "resources", "companion"), "utf8"), "owned resource");
    }
    assert.equal(await readFile(source, "utf8"), "synthetic exact candidate bytes");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
