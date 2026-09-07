// Notes retains the established Knowledge editor ceiling; Activity/Search and
// Mermaid are deferred while the outer product keeps its separate small budget.
import assert from "node:assert/strict";
import { isInvoked, measureRoute, reportBuiltRoute } from "./product-route-graph.mjs";
const spec = { label: "Knowledge Notes", entry: "/notes/App.tsx", budget: { rawBytes: 970000, gzipBytes: 325000 },
  deferred: [{ suffix: "/activity/App.tsx" }, { suffix: "/search/App.tsx" }, { name: "mermaid.core" }], forbiddenMarkers: ["mermaidAPI"] };
export function measureNotesRoute(manifest, readAsset) { return measureRoute(manifest, readAsset, spec); }
function selfTest() {
  const manifest = {
    "index.html": { file: "assets/main.js", dynamicImports: ["notes", "activity", "search"] },
    notes: { src: "src/notes/App.tsx", file: "assets/notes.js" },
    activity: { src: "src/activity/App.tsx", file: "assets/activity.js" },
    search: { src: "src/search/App.tsx", file: "assets/search.js" },
    mermaid: { name: "mermaid.core", file: "assets/mermaid.js" },
  };
  const read = () => Buffer.from("export const fixture = true;");
  assert.equal(measureNotesRoute(manifest, read).files.length, 2);
  assert.throws(() => measureNotesRoute({ ...manifest, "index.html": { ...manifest["index.html"], imports: ["mermaid"] } }, read), /eagerly loads/);
  assert.throws(() => measureNotesRoute(manifest, () => Buffer.from("mermaidAPI")), /eagerly loads/);
  assert.throws(() => measureNotesRoute({ ...manifest, activity: undefined }, read), /deferred feature/);
  console.log("Knowledge route graph rejects eager engines and missing feature entries: PASS");
}
if (isInvoked(import.meta.url)) { if (process.argv[2] === "--self-test") selfTest(); else reportBuiltRoute("knowledge", measureNotesRoute, spec); }
