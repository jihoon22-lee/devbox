import { lstatSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import process from "node:process";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const DEFAULT_ROOT = path.resolve(path.dirname(SCRIPT_PATH), "../..");
const APP_NAME_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const REQUIRED_DEPENDENCIES = ["@devbox/a11y", "@devbox/tokens"];
const REQUIRED_CSS_IMPORTS = [
  '@import "@devbox/tokens/tokens.css";',
  '@import "@devbox/a11y/styles.css";',
];

export class AccessibilityContractError extends Error {
  constructor(message) {
    super(message);
    this.name = "AccessibilityContractError";
  }
}

function fail(message) {
  throw new AccessibilityContractError(message);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function safeChild(parent, ...segments) {
  const candidate = path.resolve(parent, ...segments);
  const relative = path.relative(parent, candidate);
  if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    fail("accessibility contract path escapes the workspace");
  }
  return candidate;
}

function readText(file, label) {
  try {
    return readFileSync(file, "utf8").replace(/^\uFEFF/, "");
  } catch {
    fail(`${label} is missing or unreadable`);
  }
}

function readJson(file, label) {
  try {
    return JSON.parse(readText(file, label));
  } catch (error) {
    if (error instanceof AccessibilityContractError) throw error;
    fail(`${label} is not valid JSON`);
  }
}

function releaseApps(root) {
  const catalog = readJson(safeChild(root, "apps/catalog.json"), "app catalog");
  if (!isRecord(catalog) || !Array.isArray(catalog.apps)) fail("app catalog has an unsupported shape");

  const apps = [];
  for (const entry of catalog.apps) {
    if (!isRecord(entry) || entry.release !== true) continue;
    if (!APP_NAME_PATTERN.test(entry.id) || entry.appDir !== `apps/${entry.id}`) {
      fail("app catalog has an invalid release app entry");
    }
    apps.push(entry.id);
  }
  apps.sort();
  if (apps.length === 0 || new Set(apps).size !== apps.length) {
    fail("app catalog must declare unique release apps");
  }

  const packageApps = readdirSync(safeChild(root, "apps"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && lstatSync(safeChild(root, "apps", entry.name, "package.json"), { throwIfNoEntry: false })?.isFile())
    .map((entry) => entry.name)
    .sort();
  if (JSON.stringify(packageApps) !== JSON.stringify(apps)) {
    fail("release catalog and frontend app directories do not match");
  }
  return apps;
}

function testSources(directory) {
  const sources = [];
  const walk = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      const candidate = path.join(current, entry.name);
      if (entry.isDirectory()) walk(candidate);
      else if (entry.isFile() && /\.test\.tsx?$/.test(entry.name)) sources.push(readText(candidate, candidate));
    }
  };
  walk(directory);
  return sources;
}

function withoutBlockComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

function checkApp(root, appName) {
  const appRoot = safeChild(root, "apps", appName);
  const manifest = readJson(safeChild(appRoot, "package.json"), `${appName} package.json`);
  if (!isRecord(manifest.dependencies)) fail(`${appName} dependencies are missing`);
  for (const dependency of REQUIRED_DEPENDENCIES) {
    if (manifest.dependencies[dependency] !== "workspace:*") {
      fail(`${appName} must depend on ${dependency} via workspace:*`);
    }
  }

  const html = readText(safeChild(appRoot, "index.html"), `${appName} index.html`)
    .replace(/<!--[\s\S]*?-->/g, "");
  if (!/<html\b[^>]*\blang=["']ko-KR["'][^>]*>/i.test(html)) {
    fail(`${appName} index.html must declare lang=ko-KR`);
  }

  const css = withoutBlockComments(readText(safeChild(appRoot, "src/App.css"), `${appName} App.css`));
  let previous = -1;
  for (const requiredImport of REQUIRED_CSS_IMPORTS) {
    const index = css.indexOf(requiredImport);
    if (index < 0 || index < previous) fail(`${appName} App.css must import shared tokens then accessibility styles`);
    previous = index;
  }

  const vite = withoutBlockComments(readText(safeChild(appRoot, "vite.config.ts"), `${appName} Vite config`))
    .replace(/^[ \t]*\/\/.*$/gm, "");
  if (!/^[ \t]*manifest\s*:\s*true\b/m.test(vite)) fail(`${appName} Vite build must emit a manifest`);

  const tests = testSources(safeChild(appRoot, "src"));
  if (!tests.some((source) => (
    /import\s*\{[^}]*\bassertNoA11yViolations\b[^}]*\}\s*from\s*["']@devbox\/a11y\/testing["']/.test(source)
    && /\bassertNoA11yViolations\s*\(/.test(source)
  ))) {
    fail(`${appName} must run an axe accessibility smoke test`);
  }

  if (appName === "devbox-launcher") {
    if (!/:root\s*\{[^}]*background\s*:\s*transparent/s.test(css)
      || !/body\s*\{[^}]*background\s*:\s*transparent/s.test(css)) {
      fail("devbox-launcher must preserve transparent root and body backgrounds");
    }
  }
}

function checkSharedStyles(root) {
  const css = withoutBlockComments(readText(safeChild(root, "packages/a11y/styles.css"), "shared accessibility styles"));
  for (const fragment of [":focus-visible", "prefers-reduced-motion: reduce", "forced-colors: active"]) {
    if (!css.includes(fragment)) fail(`shared accessibility styles must include ${fragment}`);
  }
  if (/(?:^|[;{])\s*background(?:-color)?\s*:/m.test(css)) {
    fail("shared accessibility styles must not set a page background");
  }
}

export function runCheck(root = DEFAULT_ROOT) {
  let resolved;
  try {
    resolved = realpathSync(path.resolve(root));
  } catch {
    fail("accessibility contract workspace root is missing or unreadable");
  }
  if (!lstatSync(resolved).isDirectory()) fail("accessibility contract workspace root is not a directory");

  checkSharedStyles(resolved);
  const apps = releaseApps(resolved);
  for (const appName of apps) checkApp(resolved, appName);
  console.log(`Frontend accessibility contract passed for ${apps.length} apps.`);
  return apps;
}

function main(argv = process.argv.slice(2)) {
  if (!(argv.length === 0 || (argv.length === 2 && argv[0] === "--root"))) {
    fail("usage: check-frontend-accessibility.mjs [--root PATH]");
  }
  return runCheck(argv[1] ?? DEFAULT_ROOT);
}

if (path.resolve(process.argv[1] ?? "") === SCRIPT_PATH) {
  try {
    main();
  } catch (error) {
    const message = error instanceof AccessibilityContractError ? error.message : "unexpected checker failure";
    console.error(`Frontend accessibility contract failed: ${message}`);
    process.exitCode = 1;
  }
}
