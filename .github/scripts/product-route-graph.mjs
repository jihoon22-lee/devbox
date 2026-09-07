// Static closure accounting shared by the two actual product route gates.
import assert from "node:assert/strict";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";
export function measureRoute(manifest, readAsset, spec) {
  const entries = spec.rootAppExcluding
    ? (manifest["index.html"]?.dynamicImports ?? []).filter(key => manifest[key]?.name === "App"
      && !spec.rootAppExcluding.some(part => (key + (manifest[key]?.src ?? "")).includes(part))).map(key => [key, manifest[key]])
    : Object.entries(manifest).filter(([key, value]) => (value?.src ?? key).endsWith(spec.entry));
  assert.equal(entries.length, 1, `one ${spec.label} route must exist in the built manifest`);
  const visited = new Set(), files = new Set();
  function visit(key) {
    if (visited.has(key)) return;
    visited.add(key); assert.ok(visited.size <= 256, "route import graph exceeds its bound");
    const entry = manifest[key]; assert.ok(entry && typeof entry.file === "string", "route import target is missing");
    files.add(entry.file);
    for (const dependency of entry.imports ?? []) visit(dependency);
  }
  // The outer shell is part of startup even when Rollup removes the back edge.
  visit("index.html"); visit(entries[0][0]);
  for (const required of spec.deferred) {
    const matches = Object.entries(manifest).filter(([key, value]) => required.suffix
      ? (value?.src ?? key).endsWith(required.suffix) : value?.name === required.name);
    assert.equal(matches.length, 1, "required deferred feature is missing");
    assert.equal(files.has(matches[0][1].file), false, `${spec.label} eagerly loads a deferred feature`);
  }
  let rawBytes = 0, gzipBytes = 0;
  for (const file of files) {
    const bytes = readAsset(file); assert.ok(Buffer.isBuffer(bytes), "route asset must be bytes");
    for (const marker of spec.forbiddenMarkers) assert.equal(bytes.includes(Buffer.from(marker)), false, `${spec.label} eagerly loads a deferred engine`);
    rawBytes += bytes.length; gzipBytes += gzipSync(bytes, { level: 9, mtime: 0 }).length;
  }
  assert.ok(rawBytes <= spec.budget.rawBytes && gzipBytes <= spec.budget.gzipBytes, `${spec.label} exceeds its preserved ceiling`);
  return { rawBytes, gzipBytes, files: [...files].sort(), budget: spec.budget };
}
export function reportBuiltRoute(product, measure, spec) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), `../../apps/devbox-${product}/dist`);
  const realRoot = realpathSync(root);
  const manifest = JSON.parse(readFileSync(path.join(root, ".vite/manifest.json"), "utf8"));
  const report = measure(manifest, file => {
    assert.ok(/^assets\/[A-Za-z0-9._-]+\.js$/u.test(file), "route asset path is invalid");
    const resolved = realpathSync(path.join(root, file)), relative = path.relative(realRoot, resolved);
    assert.ok(relative && !relative.startsWith("..") && !path.isAbsolute(relative), "route asset leaves the built output");
    assert.ok(statSync(resolved).isFile() && statSync(resolved).size <= spec.budget.rawBytes, "route asset is invalid or too large");
    return readFileSync(resolved);
  });
  console.log(`${spec.label}: ${report.rawBytes} raw / ${report.gzipBytes} gzip bytes; required engines deferred (preserved ceilings ${spec.budget.rawBytes}/${spec.budget.gzipBytes}).`);
}
export function isInvoked(url) { return fileURLToPath(url).toLowerCase() === path.resolve(process.argv[1] ?? "").toLowerCase(); }
