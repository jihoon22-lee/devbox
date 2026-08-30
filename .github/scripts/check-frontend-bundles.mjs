import {
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import process from "node:process";
import { gzipSync } from "node:zlib";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_DIRECTORY = path.dirname(SCRIPT_PATH);
const DEFAULT_ROOT = path.resolve(SCRIPT_DIRECTORY, "../..");
const DEFAULT_CONFIG = path.join(SCRIPT_DIRECTORY, "frontend-bundle-budgets.json");
const APP_NAME_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const MODULE_SCRIPT_PATTERN = /<script\b(?:[^"'<>]|"[^"]*"|'[^']*')*>/gi;
const CONTROL_CHARACTER_PATTERN = /[\u0000-\u001f\u007f]/;

export class BundleCheckError extends Error {
  constructor(message) {
    super(message);
    this.name = "BundleCheckError";
  }
}

function fail(message) {
  throw new BundleCheckError(message);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isWhitespace(character) {
  return character === " " || character === "\t" || character === "\n" || character === "\r" || character === "\f";
}

function isPortableAbsolute(value) {
  return path.isAbsolute(value) || /^[a-zA-Z]:[\\/]/.test(value) || value.startsWith("\\\\");
}

function isInside(parent, child) {
  const relative = path.relative(parent, child);
  return relative === "" || (!relative.startsWith(".." + path.sep) && relative !== ".." && !path.isAbsolute(relative));
}

function normalizePortablePath(value) {
  return value.replaceAll("\\", "/");
}

function compareStrings(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function assertSafeRelativePath(value, label) {
  if (typeof value !== "string" || value.length === 0 || CONTROL_CHARACTER_PATTERN.test(value)) {
    fail(`${label} is invalid`);
  }

  const portable = normalizePortablePath(value);
  if (isPortableAbsolute(portable)) {
    fail(`${label} escapes the expected output directory`);
  }

  if (portable.split("/").some((component) => component === "..")) {
    fail(`${label} contains a path traversal`);
  }

  return portable;
}

function resolveContained(parent, relative, label) {
  const safeRelative = assertSafeRelativePath(relative, label);
  const candidate = path.resolve(parent, safeRelative);
  if (!isInside(parent, candidate)) {
    fail(`${label} escapes the expected output directory`);
  }
  return candidate;
}

function readConfig(configPath) {
  let source;
  try {
    source = readFileSync(configPath, "utf8");
  } catch {
    fail("frontend bundle budget config is missing or unreadable");
  }

  let parsed;
  try {
    parsed = JSON.parse(source.replace(/^\uFEFF/, ""));
  } catch {
    fail("frontend bundle budget config is not valid JSON");
  }

  if (!isRecord(parsed) || Object.keys(parsed).some((key) => !["schemaVersion", "apps"].includes(key))) {
    fail("frontend bundle budget config has an unsupported shape");
  }
  if (parsed.schemaVersion !== 1 || !isRecord(parsed.apps) || Object.keys(parsed.apps).length === 0) {
    fail("frontend bundle budget config must declare schemaVersion 1 and at least one app");
  }

  const apps = {};
  for (const appName of Object.keys(parsed.apps).sort()) {
    if (!APP_NAME_PATTERN.test(appName)) {
      fail(`frontend bundle budget config has an invalid app name: ${appName}`);
    }

    const appConfig = parsed.apps[appName];
    if (!isRecord(appConfig) || Object.keys(appConfig).some((key) => !["dist", "rawBytes", "gzipBytes"].includes(key))) {
      fail(`frontend bundle budget config for ${appName} has an unsupported shape`);
    }

    const { dist, rawBytes, gzipBytes } = appConfig;
    if (typeof dist !== "string" || dist.trim() === "" || CONTROL_CHARACTER_PATTERN.test(dist)) {
      fail(`frontend bundle budget config for ${appName} has an invalid dist path`);
    }
    assertSafeRelativePath(dist, `frontend bundle budget dist path for ${appName}`);
    if (normalizePortablePath(dist) !== `apps/${appName}/dist`) {
      fail(`frontend bundle budget config for ${appName} must use apps/${appName}/dist`);
    }
    for (const [name, value] of [["rawBytes", rawBytes], ["gzipBytes", gzipBytes]]) {
      if (!Number.isSafeInteger(value) || value < 0) {
        fail(`frontend bundle budget config for ${appName} has an invalid ${name} budget`);
      }
    }

    apps[appName] = { dist, rawBytes, gzipBytes };
  }

  return apps;
}

function readReleaseCatalogApps(rootPath) {
  const catalogPath = path.join(rootPath, "apps", "catalog.json");
  let source;
  try {
    source = readFileSync(catalogPath, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") {
      fail("app catalog is missing while validating frontend bundle coverage");
    }
    fail("app catalog is unreadable while validating frontend bundle coverage");
  }

  let catalog;
  try {
    catalog = JSON.parse(source.replace(/^\uFEFF/, ""));
  } catch {
    fail("app catalog is not valid JSON while validating frontend bundle coverage");
  }
  if (!isRecord(catalog) || !Array.isArray(catalog.apps)) {
    fail("app catalog has an unsupported shape while validating frontend bundle coverage");
  }

  const releaseApps = [];
  for (const entry of catalog.apps) {
    if (!isRecord(entry) || entry.release !== true) continue;
    if (!APP_NAME_PATTERN.test(entry.id) || entry.appDir !== `apps/${entry.id}`) {
      fail("app catalog has an invalid release app while validating frontend bundle coverage");
    }
    releaseApps.push(entry.id);
  }
  releaseApps.sort();
  if (releaseApps.length === 0 || new Set(releaseApps).size !== releaseApps.length) {
    fail("app catalog must declare unique release apps for frontend bundle coverage");
  }
  return releaseApps;
}

function assertCatalogCoverage(rootPath, appConfigs) {
  const releaseApps = readReleaseCatalogApps(rootPath);
  const configured = Object.keys(appConfigs).sort();
  if (JSON.stringify(configured) !== JSON.stringify(releaseApps)) {
    const configuredSet = new Set(configured);
    const releaseSet = new Set(releaseApps);
    const missing = releaseApps.filter((app) => !configuredSet.has(app));
    const extra = configured.filter((app) => !releaseSet.has(app));
    fail(
      `frontend bundle budgets must cover the release catalog exactly `
      + `(missing: ${missing.join(", ") || "none"}; extra: ${extra.join(", ") || "none"})`,
    );
  }
}

function parseAttributeValue(source, start, attributeName) {
  let index = start;
  while (index < source.length && isWhitespace(source[index])) index += 1;
  if (index >= source.length || source[index] !== "=") {
    return { value: true, nextIndex: index };
  }

  index += 1;
  while (index < source.length && isWhitespace(source[index])) index += 1;
  if (index >= source.length) fail(`module script attribute ${attributeName} has no value`);

  const quote = source[index];
  if (quote === "\"" || quote === "'") {
    const valueStart = index + 1;
    const end = source.indexOf(quote, valueStart);
    if (end < 0) fail(`module script attribute ${attributeName} has an unterminated value`);
    return { value: source.slice(valueStart, end), nextIndex: end + 1 };
  }

  const valueStart = index;
  while (index < source.length && !isWhitespace(source[index])) index += 1;
  if (index === valueStart) fail(`module script attribute ${attributeName} has no value`);
  return { value: source.slice(valueStart, index), nextIndex: index };
}

function parseScriptAttributes(tag) {
  const scriptStart = tag.search(/script/i);
  if (scriptStart < 0) fail("module script opening tag is malformed");
  const opening = tag.slice(scriptStart + "script".length, -1);
  const attributes = new Map();
  let index = 0;

  while (index < opening.length) {
    while (index < opening.length && isWhitespace(opening[index])) index += 1;
    if (index >= opening.length) break;
    if (opening[index] === "/") {
      index += 1;
      while (index < opening.length && isWhitespace(opening[index])) index += 1;
      if (index < opening.length) fail("module script opening tag is malformed");
      break;
    }

    const nameStart = index;
    while (
      index < opening.length &&
      !isWhitespace(opening[index]) &&
      opening[index] !== "=" &&
      opening[index] !== "/"
    ) {
      index += 1;
    }
    if (index === nameStart) fail("module script opening tag is malformed");

    const name = opening.slice(nameStart, index).toLowerCase();
    if (attributes.has(name)) fail(`module script opening tag repeats the ${name} attribute`);
    const parsed = parseAttributeValue(opening, index, name);
    attributes.set(name, parsed.value);
    index = parsed.nextIndex;
  }

  return attributes;
}

function decodeHtmlAttribute(value) {
  return value
    .replaceAll("&quot;", '"')
    .replaceAll("&#34;", '"')
    .replaceAll("&#x22;", '"')
    .replaceAll("&#39;", "'")
    .replaceAll("&#x27;", "'")
    .replaceAll("&amp;", "&")
    .replace(/&#(x[0-9a-f]+|[0-9]+);/gi, (entity, code) => {
      const value = code.toLowerCase().startsWith("x")
        ? Number.parseInt(code.slice(1), 16)
        : Number.parseInt(code, 10);
      return Number.isSafeInteger(value) && value >= 0 && value <= 0x10ffff
        ? String.fromCodePoint(value)
        : entity;
    });
}

function extractInitialModuleSources(indexHtml) {
  const sources = [];
  const withoutComments = indexHtml.replace(/<!--[\s\S]*?-->/g, "");
  for (const match of withoutComments.matchAll(MODULE_SCRIPT_PATTERN)) {
    const attributes = parseScriptAttributes(match[0]);
    const type = attributes.get("type");
    if (typeof type !== "string" || type.trim().toLowerCase() !== "module") continue;
    if (!attributes.has("src")) continue;

    const src = attributes.get("src");
    if (typeof src !== "string" || src.trim() === "") {
      fail("initial module script has an empty src attribute");
    }
    sources.push(decodeHtmlAttribute(src.trim()));
  }

  if (sources.length === 0) fail("initial module script is missing from index.html");
  return sources;
}

function decodeAssetPath(source, label) {
  const value = source.trim();
  if (value.length === 0 || CONTROL_CHARACTER_PATTERN.test(value)) fail(`${label} is invalid`);
  if (value.startsWith("//") || /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(value)) {
    fail(`${label} is not a local output path`);
  }

  const pathPart = value.split(/[?#]/, 1)[0];
  if (!pathPart) fail(`${label} is invalid`);

  let decoded;
  try {
    decoded = decodeURIComponent(pathPart);
  } catch {
    fail(`${label} has malformed URL encoding`);
  }
  const portable = normalizePortablePath(decoded);
  if (portable.startsWith("//") || /^[a-zA-Z]:[\\/]/.test(portable)) {
    fail(`${label} is not a local output path`);
  }
  const relative = portable.replace(/^\/+/, "");
  if (!relative) fail(`${label} is invalid`);
  return relative;
}

function resolveExistingFile(parent, candidate, label) {
  let stat;
  try {
    stat = lstatSync(candidate);
  } catch (error) {
    if (error?.code === "ENOENT") fail(`${label} is missing`);
    fail(`${label} could not be inspected`);
  }
  if (!stat.isFile() && !stat.isSymbolicLink()) fail(`${label} is not a regular file`);

  let real;
  try {
    real = realpathSync(candidate);
  } catch {
    fail(`${label} could not be resolved`);
  }
  if (!isInside(parent, real)) fail(`${label} escapes the expected output directory`);
  let realStat;
  try {
    realStat = lstatSync(real);
  } catch {
    fail(`${label} could not be inspected`);
  }
  if (!realStat.isFile()) fail(`${label} is not a regular file`);
  return real;
}

function readExistingFile(parent, candidate, label) {
  const real = resolveExistingFile(parent, candidate, label);
  try {
    return { path: real, bytes: readFileSync(real) };
  } catch {
    fail(`${label} could not be read`);
  }
}

function metrics(bytes) {
  return {
    rawBytes: BigInt(bytes.length),
    gzipBytes: BigInt(gzipSync(bytes, { level: 9, mtime: 0 }).length),
  };
}

function addMetrics(left, right) {
  return {
    rawBytes: left.rawBytes + right.rawBytes,
    gzipBytes: left.gzipBytes + right.gzipBytes,
  };
}

function zeroMetrics() {
  return { rawBytes: 0n, gzipBytes: 0n };
}

function relativeDisplay(parent, child) {
  return path.relative(parent, child).split(path.sep).join("/") || ".";
}

function discoverLazyChunks(distPath, initialFiles) {
  const chunks = [];
  const walk = (directory) => {
    let entries;
    try {
      entries = readdirSync(directory, { withFileTypes: true });
    } catch {
      fail("frontend output could not be enumerated");
    }

    entries.sort((left, right) => compareStrings(left.name, right.name));
    for (const entry of entries) {
      const candidate = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(candidate);
        continue;
      }
      if (entry.isSymbolicLink()) {
        let real;
        try {
          real = realpathSync(candidate);
        } catch {
          fail(`frontend output entry ${relativeDisplay(distPath, candidate)} could not be resolved`);
        }
        if (!isInside(distPath, real)) {
          fail(`frontend output entry ${relativeDisplay(distPath, candidate)} escapes the output directory`);
        }
        fail(`frontend output entry ${relativeDisplay(distPath, candidate)} uses an unsupported symlink`);
      }
      if (!entry.isFile() || path.extname(entry.name).toLowerCase() !== ".js") continue;

      const real = resolveExistingFile(distPath, candidate, `frontend output entry ${relativeDisplay(distPath, candidate)}`);
      if (initialFiles.has(real)) continue;
      const bytes = readExistingFile(distPath, candidate, `lazy chunk ${relativeDisplay(distPath, candidate)}`).bytes;
      chunks.push({
        path: relativeDisplay(distPath, candidate),
        ...metrics(bytes),
      });
    }
  };

  walk(distPath);
  chunks.sort((left, right) => compareStrings(left.path, right.path));
  return chunks;
}

function decodeUtf8(bytes, label) {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    fail(`${label} is not valid UTF-8`);
  }
}

function readViteManifest(distPath, appName) {
  const relative = ".vite/manifest.json";
  const candidate = resolveContained(distPath, relative, `${appName} Vite manifest path`);
  const source = readExistingFile(distPath, candidate, `${appName} Vite manifest`);
  let parsed;
  try {
    parsed = JSON.parse(decodeUtf8(source.bytes, `${appName} Vite manifest`).replace(/^\uFEFF/, ""));
  } catch (error) {
    if (error instanceof BundleCheckError) throw error;
    fail(`${appName} Vite manifest is not valid JSON`);
  }
  if (!isRecord(parsed) || Object.keys(parsed).length === 0) {
    fail(`${appName} Vite manifest has an unsupported shape`);
  }

  const records = new Map();
  const byFile = new Map();
  for (const [key, value] of Object.entries(parsed)) {
    if (key.length === 0 || CONTROL_CHARACTER_PATTERN.test(key) || !isRecord(value)) {
      fail(`${appName} Vite manifest has an invalid chunk record`);
    }
    const allowed = new Set([
      "assets", "css", "dynamicImports", "file", "imports", "isDynamicEntry",
      "isEntry", "name", "names", "src",
    ]);
    if (Object.keys(value).some((field) => !allowed.has(field))) {
      fail(`${appName} Vite manifest chunk ${key} has an unsupported shape`);
    }
    const file = assertSafeRelativePath(value.file, `${appName} Vite manifest file for ${key}`);
    if (path.extname(file).toLowerCase() !== ".js") {
      // CSS and copied assets can have manifest rows, but they are not part of
      // this JavaScript budget graph. They are still path-validated above.
      continue;
    }
    const imports = value.imports ?? [];
    const dynamicImports = value.dynamicImports ?? [];
    if (
      !Array.isArray(imports)
      || imports.some((item) => typeof item !== "string" || item.length === 0 || CONTROL_CHARACTER_PATTERN.test(item))
      || !Array.isArray(dynamicImports)
      || dynamicImports.some((item) => typeof item !== "string" || item.length === 0 || CONTROL_CHARACTER_PATTERN.test(item))
    ) {
      fail(`${appName} Vite manifest chunk ${key} has invalid imports`);
    }
    if (byFile.has(file)) {
      fail(`${appName} Vite manifest repeats output file ${file}`);
    }
    const record = { key, file, imports };
    records.set(key, record);
    byFile.set(file, record);
  }
  if (records.size === 0) fail(`${appName} Vite manifest has no JavaScript chunks`);
  return { records, byFile };
}

function collectInitialModuleEntries(distPath, appName, sources, manifest) {
  const queue = [];
  const entryKeys = new Set();
  for (const source of sources) {
    const file = decodeAssetPath(source, `${appName} initial module src`);
    const record = manifest.byFile.get(file);
    if (!record) fail(`${appName} initial module ${file} is missing from the Vite manifest`);
    if (entryKeys.has(record.key)) fail(`${appName} has a duplicate initial module entry: ${file}`);
    entryKeys.add(record.key);
    queue.push(record);
  }

  const visitedKeys = new Set();
  const initialFiles = new Set();
  const initialEntries = [];
  let initial = zeroMetrics();
  while (queue.length > 0) {
    const record = queue.shift();
    if (visitedKeys.has(record.key)) continue;
    visitedKeys.add(record.key);

    const candidate = resolveContained(distPath, record.file, `${appName} initial module path`);
    const asset = readExistingFile(distPath, candidate, `${appName} initial module ${record.file}`);
    if (initialFiles.has(asset.path)) {
      fail(`${appName} has a duplicate initial module output: ${record.file}`);
    }
    initialFiles.add(asset.path);
    const fileMetrics = metrics(asset.bytes);
    initial = addMetrics(initial, fileMetrics);
    initialEntries.push({ path: relativeDisplay(distPath, candidate), ...fileMetrics });

    for (const importedKey of record.imports) {
      const imported = manifest.records.get(importedKey);
      if (!imported) fail(`${appName} Vite manifest references missing static import ${importedKey}`);
      queue.push(imported);
    }
  }
  initialEntries.sort((left, right) => compareStrings(left.path, right.path));
  return { initialFiles, initialEntries, initial };
}

function checkApp(rootPath, appName, appConfig) {
  const root = realpathSync(rootPath);
  const distCandidate = resolveContained(root, appConfig.dist, `${appName} output path`);

  let distStat;
  try {
    distStat = lstatSync(distCandidate);
  } catch (error) {
    if (error?.code === "ENOENT") fail(`${appName} frontend output is missing`);
    fail(`${appName} frontend output could not be inspected`);
  }
  if (!distStat.isDirectory() && !distStat.isSymbolicLink()) {
    fail(`${appName} frontend output is missing`);
  }

  const distPath = realpathSync(distCandidate);
  if (!isInside(root, distPath)) fail(`${appName} frontend output escapes the workspace`);
  let realDistStat;
  try {
    realDistStat = lstatSync(distPath);
  } catch {
    fail(`${appName} frontend output could not be inspected`);
  }
  if (!realDistStat.isDirectory()) fail(`${appName} frontend output is missing`);

  const indexCandidate = path.join(distPath, "index.html");
  const index = readExistingFile(distPath, indexCandidate, `${appName} index.html`);
  const sources = extractInitialModuleSources(decodeUtf8(index.bytes, `${appName} index.html`));
  const manifest = readViteManifest(distPath, appName);
  const { initialFiles, initialEntries, initial } = collectInitialModuleEntries(
    distPath,
    appName,
    sources,
    manifest,
  );

  const lazy = discoverLazyChunks(distPath, initialFiles);
  const lazyTotal = lazy.reduce((total, chunk) => addMetrics(total, chunk), zeroMetrics());
  const rawBudget = BigInt(appConfig.rawBytes);
  const gzipBudget = BigInt(appConfig.gzipBytes);

  return {
    appName,
    initialEntries,
    initial,
    lazy,
    lazyTotal,
    rawBudget,
    gzipBudget,
    violations: [
      ...(initial.rawBytes > rawBudget
        ? [`${appName} initial raw budget exceeded: ${initial.rawBytes} > ${rawBudget} bytes`]
        : []),
      ...(initial.gzipBytes > gzipBudget
        ? [`${appName} initial gzip budget exceeded: ${initial.gzipBytes} > ${gzipBudget} bytes`]
        : []),
    ],
  };
}

function formatMetrics(value) {
  return `raw=${value.rawBytes} bytes, gzip=${value.gzipBytes} bytes`;
}

function printReport(report) {
  const initialNames = report.initialEntries.map((entry) => entry.path).join(", ");
  console.log(`${report.appName}: initial module entries: ${initialNames}`);
  console.log(
    `${report.appName}: initial ${formatMetrics(report.initial)} ` +
      `(budgets raw<=${report.rawBudget} bytes, gzip<=${report.gzipBudget} bytes)`,
  );
  if (report.lazy.length === 0) {
    console.log(`${report.appName}: lazy JS chunks (excluded): none`);
    return;
  }

  console.log(
    `${report.appName}: lazy JS chunks (excluded): ${report.lazy.length}, ${formatMetrics(report.lazyTotal)}`,
  );
  const displayed = [...report.lazy]
    .sort((left, right) => {
      if (left.rawBytes !== right.rawBytes) return left.rawBytes > right.rawBytes ? -1 : 1;
      return compareStrings(left.path, right.path);
    })
    .slice(0, 10);
  for (const chunk of displayed) {
    console.log(`  ${chunk.path}: ${formatMetrics(chunk)}`);
  }
  if (displayed.length < report.lazy.length) {
    console.log(`  … ${report.lazy.length - displayed.length} smaller lazy chunks omitted`);
  }
}

function parseScope(value) {
  if (value !== "all" && value !== "apps" && value !== "none") {
    fail(`unsupported frontend scope: ${value ?? "(missing)"}`);
  }
  return value;
}

function parseArguments(argv) {
  const options = {
    scope: null,
    apps: null,
    root: process.env.FRONTEND_BUNDLE_ROOT ?? DEFAULT_ROOT,
    config: process.env.FRONTEND_BUNDLE_CONFIG ?? DEFAULT_CONFIG,
  };
  const positional = [];

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith("--")) {
      positional.push(argument);
      continue;
    }

    const key = argument.slice(2);
    if (!["scope", "apps", "root", "config"].includes(key)) fail(`unsupported option: --${key}`);
    const value = argv[index + 1];
    if (value === undefined || value.startsWith("--")) fail(`option --${key} requires a value`);
    options[key] = value;
    index += 1;
  }

  if (options.scope === null) options.scope = positional.shift() ?? process.env.FRONTEND_SCOPE ?? null;
  if (options.apps === null) {
    options.apps = positional.length > 0 ? positional.join(" ") : process.env.FRONTEND_APPS ?? "";
  } else if (positional.length > 0) {
    fail("too many positional arguments");
  }
  options.scope = parseScope(options.scope);
  return options;
}

function selectedApps(scope, appConfigs, value) {
  const configured = Object.keys(appConfigs).sort();
  if (scope === "all") return configured;
  if (scope === "none") return [];

  const requested = new Set(value.replaceAll(",", " ").split(/\s+/u).filter(Boolean));
  const missing = [...requested].filter((appName) => !Object.hasOwn(appConfigs, appName)).sort();
  if (missing.length > 0) {
    fail(`frontend bundle budgets are missing selected apps: ${missing.join(", ")}`);
  }
  return configured.filter((appName) => requested.has(appName));
}

export function runCheck({ scope, frontendApps = "", root = DEFAULT_ROOT, config = DEFAULT_CONFIG } = {}) {
  const parsedScope = parseScope(scope);
  if (parsedScope === "none") {
    console.log("No frontend bundle budgets to check (scope=none).");
    return [];
  }

  let rootPath;
  try {
    rootPath = realpathSync(path.resolve(root));
  } catch {
    fail("frontend bundle workspace root is missing or unreadable");
  }
  let rootStat;
  try {
    rootStat = lstatSync(rootPath);
  } catch {
    fail("frontend bundle workspace root could not be inspected");
  }
  if (!rootStat.isDirectory()) fail("frontend bundle workspace root is not a directory");

  const configPath = path.resolve(config);
  const appConfigs = readConfig(configPath);
  assertCatalogCoverage(rootPath, appConfigs);
  const apps = selectedApps(parsedScope, appConfigs, frontendApps);
  if (apps.length === 0) {
    console.log("No configured frontend apps selected; nothing to check.");
    return [];
  }

  const reports = [];
  for (const appName of apps) {
    const report = checkApp(rootPath, appName, appConfigs[appName]);
    reports.push(report);
    printReport(report);
  }

  const violations = reports.flatMap((report) => report.violations);
  if (violations.length > 0) fail(violations.join("; "));
  console.log(`Frontend bundle budgets passed for ${apps.length} app${apps.length === 1 ? "" : "s"}.`);
  return reports;
}

export function main(argv = process.argv.slice(2)) {
  const options = parseArguments(argv);
  return runCheck({
    scope: options.scope,
    frontendApps: options.apps,
    root: options.root,
    config: options.config,
  });
}

if (path.resolve(process.argv[1] ?? "") === SCRIPT_PATH) {
  try {
    main();
  } catch (error) {
    const message = error instanceof BundleCheckError ? error.message : "unexpected checker failure";
    console.error(`Frontend bundle budget check failed: ${message}`);
    process.exitCode = 1;
  }
}
