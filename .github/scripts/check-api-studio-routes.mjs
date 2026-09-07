// Measure the active Requests closure, not only its outer lazy shell. Keep the
// pre-existing v0.7 API Playground ceiling; do not raise the shell's own budget.
import assert from "node:assert/strict";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";
const BUDGET = { rawBytes: 685000, gzipBytes: 205000 };
export function measureRequestsRoute(manifest, readAsset) {
  const entries = manifest["index.html"]?.dynamicImports ?? [];
  const requests = entries.filter(key => manifest[key]?.name === "App" && !["webhooks/", "transforms/"].some(part => (key + (manifest[key]?.src ?? "")).includes(part)));
  assert.equal(requests.length, 1, "one Requests route must exist in the built manifest");
  const visited = new Set(), files = new Set();
  function visit(key) {
    if (visited.has(key)) return; visited.add(key);
    assert.ok(visited.size <= 256, "Requests import graph exceeds its bound");
    const entry = manifest[key]; assert.ok(entry && typeof entry.file === "string", "Requests import target is missing");
    files.add(entry.file);
    for (const dependency of entry.imports ?? []) visit(dependency);
  }
  visit(requests[0]);
  for (const suffix of ["/requests/OpenApiImport.tsx", "/requests/ProtocolLab.tsx", "/transforms/App.tsx"]) {
    const deferred = Object.entries(manifest).filter(([key, value]) => (value?.src ?? key).endsWith(suffix));
    assert.equal(deferred.length, 1, "required deferred feature is missing");
    assert.equal(files.has(deferred[0][1].file), false, "Requests eagerly loads a deferred feature");
  }
  let rawBytes = 0, gzipBytes = 0;
  for (const file of files) {
    const bytes = readAsset(file); assert.ok(Buffer.isBuffer(bytes), "route asset must be bytes");
    assert.equal(bytes.includes(Buffer.from("tag:yaml.org")), false, "Requests eagerly loads the OpenAPI/YAML parser");
    rawBytes += bytes.length; gzipBytes += gzipSync(bytes, { level: 9, mtime: 0 }).length;
  }
  assert.ok(rawBytes <= BUDGET.rawBytes && gzipBytes <= BUDGET.gzipBytes, "Requests exceeds the preserved API ceiling");
  return { rawBytes, gzipBytes, files: [...files].sort(), budget: BUDGET };
}
function selfTest() {
  const manifest = {
    "index.html": { file: "assets/main.js", dynamicImports: ["requests", "transforms"] },
    requests: { name: "App", src: "src/requests/App.tsx", file: "assets/requests.js", imports: ["index.html"] },
    openapi: { src: "src/requests/OpenApiImport.tsx", file: "assets/openapi.js" },
    protocols: { src: "src/requests/ProtocolLab.tsx", file: "assets/protocols.js" },
    transforms: { name: "App", src: "src/transforms/App.tsx", file: "assets/transforms.js" },
  };
  const read = () => Buffer.from("export const fixture = true;");
  assert.equal(measureRequestsRoute(manifest, read).files.length, 2);
  assert.throws(() => measureRequestsRoute(manifest, () => Buffer.from("tag:yaml.org,2002")), /eagerly loads/);
  assert.throws(() => measureRequestsRoute({ ...manifest, requests: { ...manifest.requests, imports: ["openapi"] } }, read), /eagerly loads/);
  assert.throws(() => measureRequestsRoute({ ...manifest, protocols: undefined }, read), /deferred feature/);
  console.log("API Studio route graph rejects eager engines and missing feature entries: PASS");
}
const current = fileURLToPath(import.meta.url);
const invoked = path.resolve(process.argv[1] ?? "");
if (current.toLowerCase() === invoked.toLowerCase()) {
  if (process.argv[2] === "--self-test") selfTest();
  else {
    const root = path.resolve(path.dirname(current), "../../apps/devbox-api-studio/dist");
    const realRoot = realpathSync(root);
    const manifest = JSON.parse(readFileSync(path.join(root, ".vite/manifest.json"), "utf8"));
    const report = measureRequestsRoute(manifest, file => {
      assert.ok(/^assets\/[A-Za-z0-9._-]+\.js$/u.test(file), "route asset path is invalid");
      const resolved = realpathSync(path.join(root, file)); const relative = path.relative(realRoot, resolved);
      assert.ok(relative && !relative.startsWith("..") && !path.isAbsolute(relative), "route asset leaves the built output");
      assert.ok(statSync(resolved).isFile() && statSync(resolved).size <= BUDGET.rawBytes, "route asset is invalid or too large");
      return readFileSync(resolved);
    });
    console.log(`API Studio Requests: ${report.rawBytes} raw / ${report.gzipBytes} gzip bytes; OpenAPI/Protocols/Transforms deferred (preserved ceilings ${BUDGET.rawBytes}/${BUDGET.gzipBytes}).`);
  }
}
