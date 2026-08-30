import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SCRIPT = path.join(ROOT, ".github/scripts/check-frontend-bundles.mjs");

function fixtureApp(root, appName, { indexHtml, files = {}, manifest = undefined }) {
  const dist = path.join(root, "apps", appName, "dist");
  mkdirSync(dist, { recursive: true });
  if (indexHtml !== undefined) writeFileSync(path.join(dist, "index.html"), indexHtml, "utf8");
  for (const [relative, content] of Object.entries(files)) {
    const target = path.join(dist, relative);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, content);
  }
  if (manifest !== null) {
    let outputManifest = manifest;
    if (outputManifest === undefined) {
      const source = indexHtml?.match(/<script\b[^>]*\bsrc=["']([^"']+)["']/i)?.[1];
      const inferred = source
        ? source.split(/[?#]/, 1)[0].replace(/^\/+/, "")
        : Object.keys(files).find((file) => file.endsWith(".js"));
      outputManifest = inferred ? { "index.html": { file: inferred, isEntry: true } } : {};
    }
    const target = path.join(dist, ".vite", "manifest.json");
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, `${JSON.stringify(outputManifest, null, 2)}\n`, "utf8");
  }
  return dist;
}

function writeConfig(root, apps, catalogApps = Object.keys(apps)) {
  const appDirectory = path.join(root, "apps");
  mkdirSync(appDirectory, { recursive: true });
  writeFileSync(path.join(appDirectory, "catalog.json"), `${JSON.stringify({
    apps: catalogApps.map((appName) => ({
      id: appName,
      appDir: `apps/${appName}`,
      release: true,
    })),
  }, null, 2)}\n`, "utf8");
  const config = path.join(root, "budgets.json");
  writeFileSync(config, `${JSON.stringify({ schemaVersion: 1, apps }, null, 2)}\n`, "utf8");
  return config;
}

function runChecker(root, config, ...args) {
  const result = spawnSync(process.execPath, [SCRIPT, ...args, "--root", root, "--config", config], {
    cwd: ROOT,
    encoding: "utf8",
  });
  return {
    ...result,
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

function assertPassed(result, context) {
  assert.equal(result.status, 0, `${context} failed:\n${result.output}`);
}

function assertFailed(result, fragment, context) {
  assert.notEqual(result.status, 0, `${context} unexpectedly passed`);
  assert.match(result.output, new RegExp(fragment), `${context} did not report ${fragment}`);
}

const tempRoot = mkdtempSync(path.join(tmpdir(), "devbox-frontend-bundles-"));
try {
  {
    const root = path.join(tempRoot, "malformed-config");
    mkdirSync(root, { recursive: true });
    const config = path.join(path.dirname(root), "malformed.json");
    writeFileSync(config, '{"schemaVersion":1,"apps":{"code-pad":{"dist":"apps/code-pad/dist","rawBytes":"100","gzipBytes":100}}}\n', "utf8");
    assertFailed(runChecker(root, config, "all"), "invalid rawBytes", "malformed config fixture");
  }

  {
    const root = path.join(tempRoot, "pass");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" crossorigin src="/assets/main.js"></script>\n',
      files: { "assets/main.js": "console.log('main');\n" },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    const result = runChecker(root, config, "all");
    assertPassed(result, "pass fixture");
    assert.match(result.output, /code-pad: initial raw=/);
    assert.match(result.output, /lazy JS chunks \(excluded\): none/);
  }

  {
    const root = path.join(tempRoot, "over-budget");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/main.js"></script>\n',
      files: { "assets/main.js": "this initial bundle is too large\n" },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1, gzipBytes: 1000000 },
    });
    assertFailed(runChecker(root, config, "all"), "initial raw budget exceeded", "over-budget fixture");

    const gzipConfig = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1 },
    });
    assertFailed(runChecker(root, gzipConfig, "all"), "initial gzip budget exceeded", "gzip over-budget fixture");
  }

  {
    const root = path.join(tempRoot, "catalog-coverage");
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    }, ["code-pad", "knowledge-base"]);
    assertFailed(runChecker(root, config, "all"), "cover the release catalog exactly", "catalog coverage fixture");
  }

  {
    const root = path.join(tempRoot, "wrong-dist");
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/knowledge-base/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "must use apps/code-pad/dist", "canonical dist fixture");
  }

  {
    const root = path.join(tempRoot, "missing-catalog");
    mkdirSync(root, { recursive: true });
    const config = path.join(root, "budgets.json");
    writeFileSync(config, JSON.stringify({
      schemaVersion: 1,
      apps: {
        "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
      },
    }), "utf8");
    assertFailed(runChecker(root, config, "all"), "app catalog is missing", "missing catalog fixture");
  }

  {
    const root = path.join(tempRoot, "selected-app-coverage");
    mkdirSync(root, { recursive: true });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(
      runChecker(root, config, "apps", "code-pad,knowledge-base"),
      "missing selected apps",
      "selected app coverage fixture",
    );
  }

  {
    const root = path.join(tempRoot, "missing-output");
    mkdirSync(root, { recursive: true });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "frontend output is missing", "missing output fixture");
  }

  {
    const root = path.join(tempRoot, "missing-index");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", { files: { "assets/main.js": "main\n" } });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "index\.html is missing", "missing index fixture");
  }

  {
    const root = path.join(tempRoot, "missing-script");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/not-built.js"></script>\n',
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "initial module .* is missing", "missing script fixture");
  }

  {
    const root = path.join(tempRoot, "missing-manifest");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/main.js"></script>\n',
      files: { "assets/main.js": "main\n" },
      manifest: null,
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "Vite manifest is missing", "missing manifest fixture");
  }

  {
    const root = path.join(tempRoot, "duplicate");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml:
        '<script type="module" src="/assets/main.js"></script>\n' +
        '<script src="/assets/main.js" type="module"></script>\n',
      files: { "assets/main.js": "main\n" },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "duplicate initial module entry", "duplicate fixture");
  }

  {
    const root = path.join(tempRoot, "path-escape");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/&#x2e;&#x2e;/outside.js"></script>\n',
      files: { "outside.js": "outside\n" },
      manifest: { "index.html": { file: "assets/../outside.js", isEntry: true } },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
    });
    assertFailed(runChecker(root, config, "all"), "path traversal|escapes", "path escape fixture");
  }

  {
    const root = path.join(tempRoot, "lazy");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/main.js"></script>\n',
      files: {
        "assets/main.js": "main\n",
        "assets/very-large-lazy-chunk.js": Buffer.alloc(128 * 1024, "x"),
      },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 100, gzipBytes: 100 },
    });
    const result = runChecker(root, config, "all");
    assertPassed(result, "lazy chunk fixture");
    assert.match(result.output, /lazy JS chunks \(excluded\): 1/);
    assert.match(result.output, /very-large-lazy-chunk\.js/);
  }

  {
    const root = path.join(tempRoot, "static-import-graph");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/main.js"></script>\n',
      files: {
        "assets/main.js": "entry\n",
        "assets/shared.js": "shared static dependency\n",
        "assets/lazy.js": "large lazy dependency\n",
      },
      manifest: {
        "index.html": {
          file: "assets/main.js",
          isEntry: true,
          imports: ["_shared.js"],
          dynamicImports: ["_lazy.js"],
        },
        "_shared.js": { file: "assets/shared.js" },
        "_lazy.js": { file: "assets/lazy.js", isDynamicEntry: true },
      },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 20, gzipBytes: 1000 },
    });
    const result = runChecker(root, config, "all");
    assertFailed(result, "initial raw budget exceeded", "static import graph fixture");
    assert.match(result.output, /shared\.js/);
  }

  {
    const root = path.join(tempRoot, "scope");
    mkdirSync(root, { recursive: true });
    fixtureApp(root, "code-pad", {
      indexHtml: '<script type="module" src="/assets/code-pad.js"></script>\n',
      files: { "assets/code-pad.js": "code-pad\n" },
    });
    fixtureApp(root, "knowledge-base", {
      indexHtml: '<script type="module" src="/assets/knowledge-base.js"></script>\n',
      files: { "assets/knowledge-base.js": "knowledge-base\n" },
    });
    const config = writeConfig(root, {
      "code-pad": { dist: "apps/code-pad/dist", rawBytes: 1000, gzipBytes: 1000 },
      "knowledge-base": { dist: "apps/knowledge-base/dist", rawBytes: 1000, gzipBytes: 1000 },
    });

    const selected = runChecker(root, config, "apps", "knowledge-base");
    assertPassed(selected, "app scope fixture");
    assert.match(selected.output, /knowledge-base: initial raw=/);
    assert.doesNotMatch(selected.output, /code-pad: initial raw=/);

    const all = runChecker(root, config, "all");
    assertPassed(all, "all scope fixture");
    assert.match(all.output, /code-pad: initial raw=/);
    assert.match(all.output, /knowledge-base: initial raw=/);

    const none = runChecker(path.join(tempRoot, "does-not-exist"), path.join(tempRoot, "does-not-exist.json"), "none");
    assertPassed(none, "none scope fixture");
    assert.match(none.output, /scope=none/);
  }
} finally {
  rmSync(tempRoot, { recursive: true, force: true });
}

console.log("frontend bundle checker fixture tests passed");
