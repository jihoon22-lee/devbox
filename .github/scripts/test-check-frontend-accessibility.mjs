import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SCRIPT = path.join(ROOT, ".github/scripts/check-frontend-accessibility.mjs");

function write(file, value) {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, value, "utf8");
}

function fixture(root, overrides = {}) {
  const app = path.join(root, "apps/sample-app");
  write(
    path.join(root, "packages/a11y/styles.css"),
    overrides.sharedCss ?? ":focus-visible {}\n@media (prefers-reduced-motion: reduce) {}\n@media (forced-colors: active) {}\n",
  );
  write(path.join(root, "apps/catalog.json"), JSON.stringify({ apps: [{ id: "sample-app", appDir: "apps/sample-app", release: true }] }));
  write(path.join(app, "package.json"), JSON.stringify(overrides.package ?? {
    dependencies: { "@devbox/a11y": "workspace:*", "@devbox/tokens": "workspace:*" },
  }));
  write(path.join(app, "index.html"), overrides.html ?? '<html lang="ko-KR"></html>');
  write(path.join(app, "src/App.css"), overrides.css ?? '@import "@devbox/tokens/tokens.css";\n@import "@devbox/a11y/styles.css";\n');
  write(path.join(app, "vite.config.ts"), overrides.vite ?? "export default {\n  build: {\n    manifest: true,\n  },\n};\n");
  write(path.join(app, "src/App.test.tsx"), overrides.test ?? 'import { assertNoA11yViolations } from "@devbox/a11y/testing";\nvoid assertNoA11yViolations(document);\n');
}

function run(root) {
  const result = spawnSync(process.execPath, [SCRIPT, "--root", root], { cwd: ROOT, encoding: "utf8" });
  return { ...result, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

function expectFailure(root, fragment) {
  const result = run(root);
  assert.notEqual(result.status, 0, `fixture unexpectedly passed: ${result.output}`);
  assert.match(result.output, new RegExp(fragment));
}

const temp = mkdtempSync(path.join(tmpdir(), "devbox-a11y-contract-"));
try {
  const passing = path.join(temp, "passing");
  fixture(passing);
  assert.equal(run(passing).status, 0);

  const language = path.join(temp, "language");
  fixture(language, { html: '<html lang="en"></html>' });
  expectFailure(language, "lang=ko-KR");

  const dependency = path.join(temp, "dependency");
  fixture(dependency, { package: { dependencies: { "@devbox/tokens": "workspace:*" } } });
  expectFailure(dependency, "@devbox/a11y");

  const css = path.join(temp, "css");
  fixture(css, { css: '@import "@devbox/a11y/styles.css";\n' });
  expectFailure(css, "shared tokens then accessibility styles");

  const commentedCss = path.join(temp, "commented-css");
  fixture(commentedCss, {
    css: '/* @import "@devbox/tokens/tokens.css"; */\n@import "@devbox/a11y/styles.css";\n',
  });
  expectFailure(commentedCss, "shared tokens then accessibility styles");

  const manifest = path.join(temp, "manifest");
  fixture(manifest, { vite: "export default {};\n" });
  expectFailure(manifest, "emit a manifest");

  const commentedManifest = path.join(temp, "commented-manifest");
  fixture(commentedManifest, { vite: "export default { /* manifest: true */ };\n" });
  expectFailure(commentedManifest, "emit a manifest");

  const axe = path.join(temp, "axe");
  fixture(axe, { test: "// no accessibility smoke\n" });
  expectFailure(axe, "axe accessibility smoke test");

  const commentedAxe = path.join(temp, "commented-axe");
  fixture(commentedAxe, {
    test: '// import { assertNoA11yViolations } from "@devbox/a11y/testing";\n',
  });
  expectFailure(commentedAxe, "axe accessibility smoke test");

  const background = path.join(temp, "background");
  fixture(background, {
    sharedCss: ":focus-visible {}\n@media (prefers-reduced-motion: reduce) {}\n@media (forced-colors: active) {}\nbody { background: red; }\n",
  });
  expectFailure(background, "must not set a page background");

  console.log("Frontend accessibility contract checker tests passed.");
} finally {
  rmSync(temp, { recursive: true, force: true });
}
