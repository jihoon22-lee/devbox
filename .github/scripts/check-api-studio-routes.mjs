// Preserve the v0.7 API ceiling and the outer shell's independent budget.
import assert from "node:assert/strict";
import { isInvoked, measureRoute, reportBuiltRoute } from "./product-route-graph.mjs";
const spec = { label: "API Studio Requests", rootAppExcluding: ["webhooks/", "transforms/"], budget: { rawBytes: 685000, gzipBytes: 205000 },
  deferred: [{ suffix: "/requests/OpenApiImport.tsx" }, { suffix: "/requests/ProtocolLab.tsx" }, { suffix: "/transforms/App.tsx" }], forbiddenMarkers: ["tag:yaml.org"] };
export function measureRequestsRoute(manifest, readAsset) { return measureRoute(manifest, readAsset, spec); }
function selfTest() {
  const manifest = {
    "index.html": { file: "assets/main.js", dynamicImports: ["requests", "transforms"] },
    requests: { name: "App", src: "src/requests/App.tsx", file: "assets/requests.js" },
    openapi: { src: "src/requests/OpenApiImport.tsx", file: "assets/openapi.js" },
    protocols: { src: "src/requests/ProtocolLab.tsx", file: "assets/protocols.js" },
    transforms: { src: "src/transforms/App.tsx", file: "assets/transforms.js" },
  };
  const read = () => Buffer.from("export const fixture = true;");
  assert.equal(measureRequestsRoute(manifest, read).files.length, 2);
  assert.throws(() => measureRequestsRoute(manifest, () => Buffer.from("tag:yaml.org,2002")), /eagerly loads/);
  assert.throws(() => measureRequestsRoute({ ...manifest, requests: { ...manifest.requests, imports: ["openapi"] } }, read), /eagerly loads/);
  assert.throws(() => measureRequestsRoute({ ...manifest, protocols: undefined }, read), /deferred feature/);
  console.log("API Studio route graph rejects eager engines and missing feature entries: PASS");
}
if (isInvoked(import.meta.url)) { if (process.argv[2] === "--self-test") selfTest(); else reportBuiltRoute("api-studio", measureRequestsRoute, spec); }
