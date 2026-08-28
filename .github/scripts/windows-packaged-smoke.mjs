import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  readFileSync,
  writeFileSync,
  lstatSync,
  mkdirSync,
  renameSync,
  rmSync,
  rmdirSync,
  readdirSync,
  statSync,
} from "node:fs";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { DatabaseSync } from "node:sqlite";

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
let requestedSignal = null;
let activeAcceptanceLock = null;
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    requestedSignal ??= signal;
  });
}

function fail(message) {
  const error = new Error(message);
  error.name = "AcceptanceError";
  throw error;
}

function publicErrorMessage(error, fallback) {
  if (error instanceof Error && error.name === "AcceptanceError") return error.message;
  if (error && typeof error === "object" && typeof error.code === "string") {
    return `${fallback} (${error.code})`;
  }
  return fallback;
}

function parseArgs(argv) {
  const args = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail("invalid arguments");
    args.set(key.slice(2), value);
  }
  return args;
}

function sha256File(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function sha256Text(value) {
  return createHash("sha256").update(value).digest("hex");
}

function pathExists(target) {
  try {
    lstatSync(target);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8", flag: "w" });
}

function acquireAcceptanceLock(file, runId) {
  if (pathExists(file)) fail("a packaged-acceptance lock already exists");
  writeFileSync(file, `${JSON.stringify({ schemaVersion: 1, runId })}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  activeAcceptanceLock = { file, runId };
}

function releaseAcceptanceLock() {
  if (!activeAcceptanceLock) return;
  const { file, runId } = activeAcceptanceLock;
  const lock = JSON.parse(readFileSync(file, "utf8"));
  if (lock?.schemaVersion !== 1 || lock?.runId !== runId) {
    fail("packaged-acceptance lock ownership changed");
  }
  rmSync(file);
  activeAcceptanceLock = null;
}

function powershell(script) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const result = spawnSync(
      "powershell.exe",
      ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
      { encoding: "utf8", windowsHide: true, maxBuffer: 1024 * 1024, timeout: 15_000 },
    );
    if (result.status === 0) return result.stdout.trim();
  }
  fail("bounded Windows helper failed");
}

function windowsLocalAppData() {
  const encoded = powershell(
    `$ErrorActionPreference='Stop'; ` +
      `$value=[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData); ` +
      `if([string]::IsNullOrWhiteSpace($value)){throw 'LocalApplicationData is unavailable'}; ` +
      `[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($value))`,
  );
  return Buffer.from(encoded, "base64").toString("utf8");
}

function allWindowsProcesses() {
  const output = powershell(
    `$ErrorActionPreference='Stop'; $items=@(Get-CimInstance Win32_Process -ErrorAction Stop); ` +
      `$items | ForEach-Object { [pscustomobject]@{Pid=[int]$_.ProcessId;ParentPid=[int]$_.ParentProcessId;` +
      `Created=[string]$_.CreationDate;Name=[string]$_.Name;Path=[string]$_.ExecutablePath} } | ConvertTo-Json -Compress`,
  );
  if (!output) fail("Windows process inventory was empty");
  const parsed = JSON.parse(output);
  const items = Array.isArray(parsed) ? parsed : [parsed];
  if (
    items.length === 0 ||
    items.some(
      (item) =>
        !Number.isSafeInteger(item?.Pid) ||
        item.Pid < 0 ||
        !Number.isSafeInteger(item?.ParentPid) ||
        item.ParentPid < 0 ||
        typeof item?.Created !== "string" ||
        item.Created.length === 0 ||
        typeof item?.Name !== "string" ||
        item.Name.length === 0 ||
        typeof item?.Path !== "string",
    )
  ) {
    fail("Windows process inventory was incomplete");
  }
  return items;
}

function assertCompleteProcessIdentity(item) {
  if (item.Created.trim().length === 0 || item.Name.trim().length === 0 || item.Path.trim().length === 0) {
    fail("Windows process identity was incomplete");
  }
  return item;
}

function imageProcesses(imageName) {
  return allWindowsProcesses()
    .filter((item) => item.Name.toLowerCase() === imageName.toLowerCase())
    .map(assertCompleteProcessIdentity);
}

function matchingWindowsProcesses(imageNames) {
  const expected = new Set(imageNames.map((name) => name.toLowerCase()));
  return allWindowsProcesses()
    .filter((item) => expected.has(item.Name.toLowerCase()))
    .map(assertCompleteProcessIdentity);
}

function descendantIdentities(rootIdentity) {
  if (!rootIdentity) return [];
  const all = allWindowsProcesses();
  const root = all.find(
    (item) =>
      item.Pid === rootIdentity?.Pid &&
      item.Created === rootIdentity.Created &&
      item.Name === rootIdentity.Name &&
      item.Path === rootIdentity.Path,
  );
  if (!root) return [];
  const owned = new Set([root.Pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const item of all) {
      if (!owned.has(item.Pid) && owned.has(item.ParentPid)) {
        assertCompleteProcessIdentity(item);
        owned.add(item.Pid);
        changed = true;
      }
    }
  }
  return all.filter((item) => item.Pid !== root.Pid && owned.has(item.Pid));
}

// A child can outlive its root between the final 100 ms tracking sample and
// the Node exit event. Reconstruct only newly observed parent chains from the
// pre-launch snapshot. These candidates are never treated as owned or killed:
// they only make cleanup uncertain, preserve generated data, and block release.
function potentialNewDescendants(rootIdentity, baseline) {
  if (!rootIdentity) return [];
  const baselineIdentities = new Set(
    baseline.map((item) => `${item.Pid}:${item.Created}:${item.Name}:${item.Path}`),
  );
  const candidates = allWindowsProcesses().filter(
    (item) => !baselineIdentities.has(`${item.Pid}:${item.Created}:${item.Name}:${item.Path}`),
  );
  const potential = new Set([rootIdentity.Pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const item of candidates) {
      if (!potential.has(item.Pid) && potential.has(item.ParentPid)) {
        assertCompleteProcessIdentity(item);
        potential.add(item.Pid);
        changed = true;
      }
    }
  }
  return candidates.filter((item) => item.Pid !== rootIdentity.Pid && potential.has(item.Pid));
}

function survivingIdentities(identities) {
  const current = new Map(allWindowsProcesses().map((item) => [item.Pid, item]));
  return identities.filter((identity) => {
    const item = current.get(identity.Pid);
    if (item) assertCompleteProcessIdentity(item);
    return item && item.Created === identity.Created && item.Name === identity.Name && item.Path === identity.Path;
  });
}

function mergeIdentities(...collections) {
  const merged = new Map();
  for (const identity of collections.flat()) {
    merged.set(`${identity.Pid}:${identity.Created}:${identity.Path}`, identity);
  }
  return [...merged.values()];
}

function processState(pid) {
  const output = powershell(
    `$p=Get-Process -Id ${Number(pid)} -ErrorAction SilentlyContinue; ` +
      `if($p){[pscustomobject]@{Pid=[int]$p.Id;Responding=[bool]$p.Responding;` +
      `Title=[string]$p.MainWindowTitle;StartTime=$p.StartTime.ToUniversalTime().ToString('o')} | ConvertTo-Json -Compress}`,
  );
  return output ? JSON.parse(output) : null;
}

function nativeWindowState(pid, minimize = false) {
  const output = powershell(
    `Add-Type -Namespace DevboxAcceptance -Name NativeMethods -MemberDefinition '` +
      `[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow(); ` +
      `[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr hWnd, out uint processId); ` +
      `[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr hWnd); ` +
      `[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr hWnd); ` +
      `[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool ShowWindowAsync(System.IntPtr hWnd, int command);'; ` +
      `$p=Get-Process -Id ${Number(pid)} -ErrorAction SilentlyContinue; if(!$p){exit 0}; ` +
      `$handle=$p.MainWindowHandle; ` +
      (minimize ? `if($handle -ne [IntPtr]::Zero){[void][DevboxAcceptance.NativeMethods]::ShowWindowAsync($handle,6)}; ` : ``) +
      `$foreground=[DevboxAcceptance.NativeMethods]::GetForegroundWindow(); [uint32]$foregroundPid=0; ` +
      `if($foreground -ne [IntPtr]::Zero){[void][DevboxAcceptance.NativeMethods]::GetWindowThreadProcessId($foreground,[ref]$foregroundPid)}; ` +
      `[pscustomobject]@{HasHandle=($handle -ne [IntPtr]::Zero);` +
      `Visible=$(if($handle -eq [IntPtr]::Zero){$false}else{[DevboxAcceptance.NativeMethods]::IsWindowVisible($handle)});` +
      `Minimized=$(if($handle -eq [IntPtr]::Zero){$false}else{[DevboxAcceptance.NativeMethods]::IsIconic($handle)});` +
      `ForegroundPid=[int]$foregroundPid} | ConvertTo-Json -Compress`,
  );
  return output ? JSON.parse(output) : null;
}

async function displaceOwnedWindow(pid, allowInitiallyHidden) {
  const initial = nativeWindowState(pid);
  if (!initial) fail("packaged main window state was unavailable");
  if (!initial.HasHandle || !initial.Visible) {
    if (!allowInitiallyHidden) fail("packaged main window was unexpectedly hidden");
    return { mode: "initially-hidden", displaced: true };
  }
  nativeWindowState(pid, true);
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    const current = nativeWindowState(pid);
    if (current?.Minimized && current.ForegroundPid !== pid) {
      return { mode: "minimized", displaced: true };
    }
    await sleep(100);
  }
  fail("could not displace focus from the packaged main window");
}

async function waitForOwnedForegroundWindow(pid, timeoutMilliseconds = 5_000) {
  const deadline = Date.now() + timeoutMilliseconds;
  while (Date.now() < deadline) {
    const current = nativeWindowState(pid);
    if (current?.HasHandle && current.Visible && !current.Minimized && current.ForegroundPid === pid) {
      return current;
    }
    await sleep(100);
  }
  fail("second launch did not restore the owned foreground window");
}

function listenerOwnerPids(port) {
  const output = powershell(
    `$items=@(Get-NetTCPConnection -State Listen -LocalPort ${Number(port)} -ErrorAction SilentlyContinue | ` +
      `Select-Object -ExpandProperty OwningProcess -Unique); @($items) | ConvertTo-Json -Compress`,
  );
  if (!output) return [];
  const parsed = JSON.parse(output);
  return (Array.isArray(parsed) ? parsed : [parsed]).map(Number).filter(Number.isSafeInteger);
}

function directorySummary(directory) {
  if (!pathExists(directory)) return { existed: false, itemCount: 0 };
  const metadata = lstatSync(directory);
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    fail("protected app-data path is not a plain directory");
  }
  return { existed: true, itemCount: readdirSync(directory).length };
}

function capture(stream) {
  const chunks = [];
  let bytes = 0;
  let storedBytes = 0;
  let truncated = false;
  stream.on("data", (chunk) => {
    bytes += chunk.length;
    const remaining = 65_536 - storedBytes;
    if (remaining > 0) {
      const kept = Buffer.from(chunk.subarray(0, remaining));
      chunks.push(kept);
      storedBytes += kept.length;
    }
    if (chunk.length > remaining) truncated = true;
  });
  return () => {
    const payload = Buffer.concat(chunks);
    return { bytes, sha256: sha256Text(payload), truncated };
  };
}

function sanitizedWindowsEnvironment(overrides) {
  const allowed = new Set(
    [
      "SystemRoot",
      "WINDIR",
      "ComSpec",
      "Path",
      "PATHEXT",
      "USERPROFILE",
      "HOMEDRIVE",
      "HOMEPATH",
      "LOCALAPPDATA",
      "APPDATA",
      "TEMP",
      "TMP",
      "ProgramData",
      "ProgramFiles",
      "ProgramFiles(x86)",
      "CommonProgramFiles",
      "CommonProgramFiles(x86)",
      "PROCESSOR_ARCHITECTURE",
      "PROCESSOR_IDENTIFIER",
      "NUMBER_OF_PROCESSORS",
      "OS",
      "USERNAME",
      "USERDOMAIN",
      "SESSIONNAME",
      "WSLENV",
    ].map((key) => key.toLowerCase()),
  );
  const environment = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined && allowed.has(key.toLowerCase())) environment[key] = value;
  }
  for (const [overrideKey, value] of Object.entries(overrides)) {
    for (const existingKey of Object.keys(environment)) {
      if (existingKey.toLowerCase() === overrideKey.toLowerCase()) delete environment[existingKey];
    }
    environment[overrideKey] = value;
  }
  return environment;
}

async function unusedPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close((error) => (error ? reject(error) : resolve(port)));
    });
  });
}

async function waitForCdp(port, expectedTitle, timeoutMilliseconds = 30_000) {
  const deadline = Date.now() + timeoutMilliseconds;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(1_000),
      });
      if (response.ok) {
        const payload = await response.text();
        if (payload.length > 1_048_576) fail("CDP target list exceeded its bound");
        const targets = JSON.parse(payload);
        const candidates = targets.filter((target) => target.type === "page" && target.webSocketDebuggerUrl);
        const exact = candidates.find((target) => target.title === expectedTitle);
        if (exact) return exact;
      }
    } catch {
      // The app owns this loopback port and may still be starting.
    }
    await sleep(250);
  }
  fail("packaged WebView2 CDP target did not appear");
}

class Cdp {
  constructor(url) {
    this.url = url;
    this.nextId = 1;
    this.pending = new Map();
    this.runtimeExceptions = 0;
    this.consoleErrors = 0;
    this.logErrors = 0;
  }

  async connect() {
    this.socket = new WebSocket(this.url);
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("CDP WebSocket timeout")), 10_000);
      this.socket.addEventListener(
        "open",
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
      this.socket.addEventListener(
        "error",
        () => {
          clearTimeout(timer);
          reject(new Error("CDP WebSocket failed"));
        },
        { once: true },
      );
    });
    this.socket.addEventListener("message", (event) => {
      let message;
      try {
        message = JSON.parse(String(event.data));
      } catch {
        this.runtimeExceptions += 1;
        return;
      }
      if (message.id) {
        const pending = this.pending.get(message.id);
        if (!pending) return;
        this.pending.delete(message.id);
        if (message.error) pending.reject(new Error("CDP command failed"));
        else pending.resolve(message.result);
        return;
      }
      if (message.method === "Runtime.exceptionThrown") this.runtimeExceptions += 1;
      if (message.method === "Runtime.consoleAPICalled" && message.params?.type === "error") {
        this.consoleErrors += 1;
      }
      if (message.method === "Log.entryAdded" && message.params?.entry?.level === "error") {
        this.logErrors += 1;
      }
    });
    this.socket.addEventListener("close", () => {
      for (const pending of this.pending.values()) pending.reject(new Error("CDP WebSocket closed"));
      this.pending.clear();
    });
    await Promise.all([this.send("Runtime.enable"), this.send("Log.enable")]);
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("CDP command timeout"));
      }, 15_000);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate(expression) {
    const response = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (response.exceptionDetails) fail("packaged renderer evaluation failed");
    return response.result?.value;
  }

  close() {
    this.socket?.close();
  }
}

async function waitForExit(child, timeoutMilliseconds) {
  if (child.exitCode !== null) return { exited: true, code: child.exitCode, signal: child.signalCode };
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      child.removeListener("exit", onExit);
      resolve({ exited: false, code: null, signal: null });
    }, timeoutMilliseconds);
    const onExit = (code, signal) => {
      clearTimeout(timer);
      resolve({ exited: true, code, signal });
    };
    child.once("exit", onExit);
    if (child.exitCode !== null) {
      clearTimeout(timer);
      child.removeListener("exit", onExit);
      resolve({ exited: true, code: child.exitCode, signal: child.signalCode });
    }
  });
}

async function waitForExitTracking(child, rootIdentity, timeoutMilliseconds) {
  const deadline = Date.now() + timeoutMilliseconds;
  let descendants = [];
  while (Date.now() < deadline) {
    descendants = mergeIdentities(descendants, descendantIdentities(rootIdentity));
    if (child.exitCode !== null) {
      return {
        exited: true,
        code: child.exitCode,
        signal: child.signalCode,
        descendants,
      };
    }
    await sleep(100);
  }
  return { exited: false, code: null, signal: null, descendants };
}

function terminateIdentity(identity, force) {
  const expectedCreated = Buffer.from(identity.Created, "utf8").toString("base64");
  const expectedName = Buffer.from(identity.Name, "utf8").toString("base64");
  const expectedPath = Buffer.from(identity.Path, "utf8").toString("base64");
  const action = force
    ? `Stop-Process -Id ${Number(identity.Pid)} -Force -ErrorAction Stop; $acted=$true; `
    : `$process=[Diagnostics.Process]::GetProcessById(${Number(identity.Pid)}); $acted=$process.CloseMainWindow(); `;
  const output = powershell(
    `$decode=[Text.Encoding]::UTF8; ` +
      `$expectedCreated=$decode.GetString([Convert]::FromBase64String('${expectedCreated}')); ` +
      `$expectedName=$decode.GetString([Convert]::FromBase64String('${expectedName}')); ` +
      `$expectedPath=$decode.GetString([Convert]::FromBase64String('${expectedPath}')); ` +
      `$item=Get-CimInstance Win32_Process -Filter 'ProcessId = ${Number(identity.Pid)}' -ErrorAction SilentlyContinue; ` +
      `$acted=$false; if($item -and [string]$item.CreationDate -ceq $expectedCreated -and ` +
      `[string]$item.Name -ceq $expectedName -and [string]$item.ExecutablePath -ieq $expectedPath){${action}}; ` +
      `[bool]$acted | ConvertTo-Json -Compress`,
  );
  return output === "true";
}

async function waitForProcessIdentity(pid, executable, timeoutMilliseconds = 10_000) {
  const expected = path.resolve(executable).toLowerCase();
  const deadline = Date.now() + timeoutMilliseconds;
  while (Date.now() < deadline) {
    const exact = imageProcesses(path.basename(executable)).find(
      (item) => item.Pid === pid && path.resolve(item.Path).toLowerCase() === expected,
    );
    if (exact) return exact;
    await sleep(100);
  }
  fail("owned packaged process identity was not observable");
}

async function stopOwnedProcess(identity, executable, child) {
  if (!identity) return { forced: false, alreadyExited: child.exitCode !== null, descendants: [] };
  const expected = path.resolve(executable).toLowerCase();
  if (path.resolve(identity.Path).toLowerCase() !== expected) fail("owned process path identity changed");
  let descendants = descendantIdentities(identity);
  if (survivingIdentities([identity]).length !== 1) {
    return { forced: false, alreadyExited: true, descendants };
  }

  terminateIdentity(identity, false);
  let exited = await waitForExitTracking(child, identity, 12_000);
  descendants = mergeIdentities(descendants, exited.descendants ?? []);
  if (exited.exited) return { forced: false, alreadyExited: false, descendants };

  if (survivingIdentities([identity]).length !== 1) {
    return { forced: false, alreadyExited: true, descendants };
  }
  terminateIdentity(identity, true);
  exited = await waitForExitTracking(child, identity, 12_000);
  descendants = mergeIdentities(descendants, exited.descendants ?? []);
  if (!exited.exited) fail("owned packaged process could not be cleaned up");
  return { forced: true, alreadyExited: false, descendants };
}

function removeGenerated(directory, allowedParents) {
  if (!pathExists(directory)) return;
  const resolved = path.resolve(directory);
  const allowed = allowedParents.some((parent) => path.dirname(resolved).toLowerCase() === path.resolve(parent).toLowerCase());
  if (!allowed) fail("refusing to remove an unexpected app-data path");
  rmSync(resolved, { recursive: true, force: false, maxRetries: 3, retryDelay: 250 });
}

const OWNER_MARKER = ".devbox-acceptance-owner.json";
const KNOWLEDGE_LAYOUT = ["Projects", "Notes", "Journal", "Reference", "Archive"];

function isOwnedDirectory(directory, token) {
  if (!pathExists(directory)) return false;
  const metadata = lstatSync(directory);
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) return false;
  const marker = path.join(directory, OWNER_MARKER);
  if (!pathExists(marker) || lstatSync(marker).isSymbolicLink()) return false;
  try {
    const value = JSON.parse(readFileSync(marker, "utf8"));
    return value?.schemaVersion === 1 && value?.token === token;
  } catch {
    return false;
  }
}

function assertNoStaleAcceptancePaths(directory, actualLocalAppData) {
  const parent = path.dirname(directory);
  if (parent.toLowerCase() !== path.resolve(actualLocalAppData).toLowerCase()) {
    fail("protected app-data path escaped its parent");
  }
  const base = path.basename(directory);
  if (pathExists(directory) && pathExists(path.join(directory, OWNER_MARKER))) {
    fail("stale packaged-acceptance current data requires recovery");
  }
  const stalePrefixes = [
    `${base}.devbox-v050-`,
    `${base}.devbox-generated-v050-`,
    `${base}.devbox-staging-v050-`,
  ];
  if (readdirSync(parent).some((entry) => stalePrefixes.some((prefix) => entry.startsWith(prefix)))) {
    fail("stale packaged-acceptance data requires recovery");
  }
}

function prepareKnowledgeRoot(appDataDirectory, knowledgeRoot) {
  for (const subdirectory of KNOWLEDGE_LAYOUT) {
    mkdirSync(path.join(knowledgeRoot, subdirectory), { recursive: true });
  }
  const databaseFile = path.join(appDataDirectory, "data.db");
  if (pathExists(databaseFile)) fail("isolated Knowledge database already exists");
  const database = new DatabaseSync(databaseFile);
  try {
    database.exec(
      "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    );
    database.prepare("INSERT INTO settings (key, value) VALUES (?, ?)").run("root", knowledgeRoot);
    const selected = database.prepare("SELECT value FROM settings WHERE key = ?").get("root");
    if (selected?.value !== knowledgeRoot) fail("isolated Knowledge root read-back failed");
  } finally {
    database.close();
  }
}

function assertPlainPathAndAncestors(target, requireDirectory) {
  const resolved = path.resolve(target);
  const parsed = path.parse(resolved);
  let current = parsed.root;
  for (const segment of resolved.slice(parsed.root.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, segment);
    if (!pathExists(current)) break;
    const metadata = lstatSync(current);
    if (metadata.isSymbolicLink()) fail("acceptance path contains a linked component");
  }
  if (requireDirectory) {
    if (!pathExists(resolved) || !lstatSync(resolved).isDirectory()) {
      fail("required acceptance directory is unavailable");
    }
  }
}

function assertSafeConfig(config) {
  if (config.schemaVersion !== 1 || Object.keys(config).sort().join(",") !== "apps,schemaVersion") {
    fail("invalid acceptance config envelope");
  }
  if (!Array.isArray(config.apps) || config.apps.length !== 15) fail("invalid acceptance app matrix");
  const ids = new Set();
  const dataIdentifiers = new Set();
  const processNames = new Set();
  const isolatedKnowledgeApps = [];
  for (const app of config.apps) {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(app.id) || ids.has(app.id)) {
      fail("invalid or duplicate acceptance app id");
    }
    ids.add(app.id);
    if (app.isolatedKnowledgeRoot !== undefined && app.isolatedKnowledgeRoot !== true) {
      fail("invalid isolated Knowledge root contract");
    }
    if (app.isolatedKnowledgeRoot === true) isolatedKnowledgeApps.push(app.id);
    const identifiers = [app.identifier, ...(app.legacyIdentifiers ?? [])];
    if (identifiers.some((identifier) => !/^com\.(?:devbox|workbench)\.[a-z0-9]+$/.test(identifier))) {
      fail("invalid acceptance app-data identifier");
    }
    for (const identifier of identifiers) {
      const normalized = identifier.toLowerCase();
      if (dataIdentifiers.has(normalized)) fail("duplicate acceptance app-data identifier");
      dataIdentifiers.add(normalized);
    }
    if (
      !/^\d+\.\d+\.\d+$/.test(app.version) ||
      typeof app.title !== "string" ||
      app.title.length === 0 ||
      !Array.isArray(app.markers) ||
      app.markers.length === 0 ||
      app.markers.some((marker) => typeof marker !== "string" || marker.length === 0) ||
      !Array.isArray(app.probes) ||
      app.probes.length === 0
    ) {
      fail("invalid acceptance runtime contract");
    }
    for (const probe of app.probes) {
      if (
        typeof probe?.name !== "string" ||
        typeof probe?.command !== "string" ||
        !["null", "array", "object", "string", "number"].includes(probe.expectedType) ||
        (probe.requiredKeys !== undefined &&
          (!Array.isArray(probe.requiredKeys) ||
            probe.requiredKeys.length === 0 ||
            probe.requiredKeys.some((key) => typeof key !== "string" || key.length === 0)))
      ) {
        fail("invalid acceptance IPC probe contract");
      }
    }
    if (
      (app.additionalProcessNames ?? []).some(
        (name) => !/^[A-Za-z0-9 .+_-]+\.exe$/.test(name) || path.basename(name) !== name,
      )
    ) {
      fail("invalid protected process name");
    }
    const appProcessNames = new Set(
      [`${app.id}.exe`, ...(app.additionalProcessNames ?? [])].map((name) => name.toLowerCase()),
    );
    for (const normalized of appProcessNames) {
      if (processNames.has(normalized)) fail("duplicate protected process name");
      processNames.add(normalized);
    }
  }
  if (isolatedKnowledgeApps.length !== 1 || isolatedKnowledgeApps[0] !== "knowledge-base") {
    fail("Knowledge Base must use the isolated acceptance root");
  }
}

function assertSafeScratchLayout(
  configFile,
  verificationFile,
  assetsDirectory,
  outputFile,
  runtimeRoot,
  actualLocalAppData,
) {
  const scratchRoot = path.parse(runtimeRoot).root.toLowerCase();
  const localDataRoot = path.parse(actualLocalAppData).root.toLowerCase();
  if (!scratchRoot || scratchRoot === localDataRoot) fail("acceptance scratch must be off the user-data volume");
  if (
    [configFile, verificationFile, assetsDirectory, outputFile].some(
      (candidate) => path.parse(candidate).root.toLowerCase() !== scratchRoot,
    )
  ) {
    fail("acceptance inputs and output must share the scratch volume");
  }
  const relativeRuntime = path.relative(path.parse(runtimeRoot).root, runtimeRoot);
  if (relativeRuntime.split(path.sep).filter(Boolean).length < 3) {
    fail("acceptance runtime root is too broad");
  }
  if (pathExists(runtimeRoot)) fail("acceptance runtime root must be new");
  assertPlainPathAndAncestors(configFile, false);
  assertPlainPathAndAncestors(verificationFile, false);
  assertPlainPathAndAncestors(assetsDirectory, true);
  assertPlainPathAndAncestors(path.dirname(outputFile), true);
  assertPlainPathAndAncestors(path.dirname(runtimeRoot), true);
}

function writeTransactionJournal(file, appId, runId, phase, records) {
  writeJson(file, {
    schemaVersion: 1,
    appId,
    runId,
    phase,
    paths: records.map((record) => ({
      name: path.basename(record.directory),
      backup: path.basename(record.backup),
      quarantine: path.basename(record.quarantine),
      staging: path.basename(record.staging),
      originalExisted: record.originalExisted,
      backedUp: record.backedUp,
      prepared: record.prepared,
    })),
  });
}

async function runApp(app, context) {
  const executable = path.join(context.assetsDirectory, `${app.id}.exe`);
  const imageName = `${app.id}.exe`;
  const result = {
    id: app.id,
    version: app.version,
    asset: imageName,
    assetBytes: 0,
    assetSha256: "",
    originalData: {},
    status: "FAIL",
    cleanup: {},
  };
  if (!pathExists(executable)) fail(`missing packaged asset: ${imageName}`);
  result.assetBytes = statSync(executable).size;
  result.assetSha256 = sha256File(executable);
  const manifestApp = context.manifest.apps.find((entry) => entry.id === app.id && entry.version === app.version);
  const manifestAsset = manifestApp?.portable;
  if (!manifestAsset || manifestAsset.size !== result.assetBytes || manifestAsset.sha256 !== result.assetSha256) {
    fail(`manifest identity mismatch: ${imageName}`);
  }

  const preexisting = matchingWindowsProcesses(context.protectedProcessNames);
  if (preexisting.length > 0) {
    result.status = "SKIP";
    result.skipReason = "pre-existing Devbox process protected";
    result.preexistingProcessCount = preexisting.length;
    result.cleanup.releaseGateBlocked = true;
    return result;
  }

  const protectedPaths = [
    ...[app.identifier, ...(app.legacyIdentifiers ?? [])].map((identifier) =>
      path.join(context.actualLocalAppData, identifier),
    ),
    context.sharedLocalDataRoot,
  ];
  const protectedRecords = [];
  let child;
  let childIdentity;
  let secondChild;
  let secondIdentity;
  let cdp;
  let isolatedRoot;
  let transactionJournal;
  let ownedDescendants = [];
  let uncertainDescendants = [];
  let processesBeforePrimary = [];
  let processesBeforeSecond = [];
  try {
    isolatedRoot = path.join(context.runtimeRoot, app.id);
    const isolatedLocal = path.join(isolatedRoot, "local");
    const isolatedRoaming = path.join(isolatedRoot, "roaming");
    const isolatedTemp = path.join(isolatedRoot, "temp");
    mkdirSync(isolatedLocal, { recursive: true });
    mkdirSync(isolatedRoaming, { recursive: true });
    mkdirSync(isolatedTemp, { recursive: true });
    transactionJournal = path.join(context.runtimeRoot, `${app.id}-transaction.json`);
    writeTransactionJournal(transactionJournal, app.id, context.runId, "preparing", protectedRecords);
    for (const directory of protectedPaths) {
      const summary = directorySummary(directory);
      result.originalData[path.basename(directory)] = summary;
      assertNoStaleAcceptancePaths(directory, context.actualLocalAppData);
      const backup = `${directory}.devbox-v050-${context.runId}`;
      const quarantine = `${directory}.devbox-generated-v050-${context.runId}`;
      const staging = `${directory}.devbox-staging-v050-${context.runId}`;
      const record = {
        directory,
        backup,
        quarantine,
        staging,
        originalExisted: summary.existed,
        mutated: false,
        backedUp: false,
        prepared: false,
      };
      protectedRecords.push(record);
      writeTransactionJournal(transactionJournal, app.id, context.runId, "preparing", protectedRecords);
      if (pathExists(backup) || pathExists(quarantine) || pathExists(staging)) {
        fail("app-data transaction collision");
      }
      mkdirSync(staging);
      record.mutated = true;
      writeFileSync(
        path.join(staging, OWNER_MARKER),
        `${JSON.stringify({ schemaVersion: 1, token: context.runId })}\n`,
        { encoding: "utf8", flag: "wx" },
      );
      if (!isOwnedDirectory(staging, context.runId)) fail("app-data staging owner marker read-back failed");
      if (summary.existed) {
        renameSync(directory, backup);
        record.backedUp = true;
        if (pathExists(directory) || !pathExists(backup)) fail("app-data backup read-back failed");
      }
      renameSync(staging, directory);
      record.prepared = true;
      if (!isOwnedDirectory(directory, context.runId)) fail("app-data owner marker read-back failed");
      writeTransactionJournal(transactionJournal, app.id, context.runId, "prepared", protectedRecords);
    }
    if (app.isolatedKnowledgeRoot === true) {
      prepareKnowledgeRoot(
        path.join(context.actualLocalAppData, app.identifier),
        path.join(isolatedRoot, "knowledge-root"),
      );
      result.isolatedKnowledgeRoot = true;
    }
    const port = await unusedPort();
    const environment = sanitizedWindowsEnvironment({
      LOCALAPPDATA: isolatedLocal,
      APPDATA: isolatedRoaming,
      TEMP: isolatedTemp,
      TMP: isolatedTemp,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(isolatedRoot, "webview2"),
    });
    const startedAt = new Date().toISOString();
    processesBeforePrimary = allWindowsProcesses();
    child = spawn(executable, [], { env: environment, windowsHide: false, stdio: ["ignore", "pipe", "pipe"] });
    child.on("error", () => {});
    if (!child.pid) fail("packaged process did not start");
    childIdentity = await waitForProcessIdentity(child.pid, executable);
    const stdoutResult = capture(child.stdout);
    const stderrResult = capture(child.stderr);

    const target = await waitForCdp(port, app.title);
    const allowedCdpOwners = new Set([
      child.pid,
      ...descendantIdentities(childIdentity).map((item) => item.Pid),
    ]);
    const cdpOwners = listenerOwnerPids(port);
    if (cdpOwners.length !== 1 || !allowedCdpOwners.has(cdpOwners[0])) {
      fail("CDP listener ownership did not match the packaged process tree");
    }
    cdp = new Cdp(target.webSocketDebuggerUrl);
    await cdp.connect();
    const renderer = await cdp.evaluate(`(() => {
      const body = document.body?.innerText ?? '';
      return {
        readyState: document.readyState,
        title: document.title,
        bodyLength: body.length,
        bodySha256Input: body,
        rootChildren: document.querySelector('#root')?.childElementCount ?? 0,
        markers: ${JSON.stringify(app.markers ?? [])}.map((marker) => body.includes(marker)),
        hasDialog: Boolean(document.querySelector('[role="dialog"]')),
        focused: document.hasFocus()
      };
    })()`);
    result.renderer = {
      readyState: renderer.readyState,
      title: renderer.title,
      bodyLength: renderer.bodyLength,
      bodySha256: sha256Text(renderer.bodySha256Input),
      rootChildren: renderer.rootChildren,
      markers: renderer.markers,
      hasDialog: renderer.hasDialog,
      focusedBeforeSecondInstance: renderer.focused,
    };
    if (!["interactive", "complete"].includes(renderer.readyState)) fail("renderer did not become ready");
    if (renderer.title !== app.title || renderer.bodyLength === 0 || renderer.rootChildren === 0) {
      fail("packaged renderer identity failed");
    }
    if (renderer.markers.some((present) => !present)) fail("expected packaged UI marker missing");
    if (renderer.hasDialog) fail("unexpected packaged startup dialog");

    result.probes = [];
    for (const probe of app.probes ?? []) {
      const summary = await cdp.evaluate(`(async () => {
        const value = await Promise.race([
          window.__TAURI_INTERNALS__.invoke(
            ${JSON.stringify(probe.command)},
            ${JSON.stringify(probe.args ?? {})}
          ),
          new Promise((_, reject) => setTimeout(() => reject(new Error('probe timeout')), 30000))
        ]);
        if (value === null) return { type: 'null' };
        if (Array.isArray(value)) return { type: 'array', length: value.length };
        if (typeof value === 'object') return { type: 'object', keys: Object.keys(value).sort() };
        if (typeof value === 'string') return { type: 'string', length: value.length };
        if (typeof value === 'number') return { type: 'number', finite: Number.isFinite(value) };
        return { type: typeof value };
      })()`);
      result.probes.push({ name: probe.name, command: probe.command, summary });
      if (probe.expectedType && summary.type !== probe.expectedType) {
        fail(`packaged IPC probe type mismatch: ${probe.name}`);
      }
      if (probe.minimumLength !== undefined && (summary.length ?? -1) < probe.minimumLength) {
        fail(`packaged IPC probe length mismatch: ${probe.name}`);
      }
      if (
        probe.requiredKeys &&
        (!Array.isArray(summary.keys) || probe.requiredKeys.some((key) => !summary.keys.includes(key)))
      ) {
        fail(`packaged IPC probe shape mismatch: ${probe.name}`);
      }
    }

    const elapsed = Date.now() - Date.parse(startedAt);
    if (elapsed < 10_000) await sleep(10_000 - elapsed);
    const firstState = processState(child.pid);
    const acceptableWindowTitle = firstState?.Title === app.title || (app.allowHiddenWindow && firstState?.Title === "");
    if (!firstState || !firstState.Responding || !acceptableWindowTitle || child.exitCode !== null) {
      fail("packaged parent was not healthy after ten seconds");
    }
    result.firstInstance = {
      pidObserved: true,
      responding: firstState.Responding,
      title: firstState.Title,
      survivedTenSeconds: true,
    };

    result.focusDisplacement = await displaceOwnedWindow(child.pid, app.allowHiddenWindow === true);
    processesBeforeSecond = allWindowsProcesses();
    secondChild = spawn(executable, [], { env: environment, windowsHide: false, stdio: ["ignore", "pipe", "pipe"] });
    secondChild.on("error", () => {});
    if (!secondChild.pid) fail("second packaged process did not start");
    try {
      secondIdentity = await waitForProcessIdentity(secondChild.pid, executable, 1_000);
    } catch {
      fail("second packaged process identity was not observable");
    }
    const secondStdout = capture(secondChild.stdout);
    const secondStderr = capture(secondChild.stderr);
    const secondExit = await waitForExitTracking(secondChild, secondIdentity, 10_000);
    ownedDescendants = mergeIdentities(
      ownedDescendants,
      secondExit.descendants ?? [],
    );
    const trackedSecondIdentities = new Set(
      ownedDescendants.map((identity) => `${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
    );
    uncertainDescendants = mergeIdentities(
      uncertainDescendants,
      potentialNewDescendants(secondIdentity, processesBeforeSecond).filter(
        (identity) =>
          !trackedSecondIdentities.has(`${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
      ),
    );
    const postSecondInventory = imageProcesses(imageName);
    result.nativeWindowAfterSecond = await waitForOwnedForegroundWindow(child.pid);
    const focusedAfter = await cdp.evaluate("document.hasFocus()");
    const firstStateAfterSecond = processState(child.pid);
    const firstHealthyAfterSecond =
      firstStateAfterSecond?.Responding === true && firstStateAfterSecond.Title === app.title;
    result.secondInstance = {
      exitedWithinTenSeconds: secondExit.exited,
      exitCode: secondExit.code,
      firstStillAlive: child.exitCode === null,
      persistentImageCount: postSecondInventory.length,
      firstFocusedAfter: focusedAfter,
      firstRespondingAfter: firstStateAfterSecond?.Responding === true,
      firstTitleAfter: firstStateAfterSecond?.Title ?? "",
      stdout: secondStdout(),
      stderr: secondStderr(),
    };
    if (
      !secondExit.exited ||
      secondExit.code !== 0 ||
      child.exitCode !== null ||
      postSecondInventory.length !== 1 ||
      !focusedAfter ||
      !firstHealthyAfterSecond ||
      result.secondInstance.stdout.bytes !== 0 ||
      result.secondInstance.stderr.bytes !== 0
    ) {
      fail("single-instance packaged contract failed");
    }

    result.runtime = {
      exceptionsAfterAttach: cdp.runtimeExceptions,
      consoleErrorsAfterAttach: cdp.consoleErrors,
      bufferedLogErrors: cdp.logErrors,
      stdout: stdoutResult(),
      stderr: stderrResult(),
    };
    if (cdp.runtimeExceptions !== 0 || cdp.consoleErrors !== 0 || cdp.logErrors !== 0) {
      fail("packaged renderer raised a runtime, console, or buffered log error");
    }
    if (result.runtime.stdout.bytes !== 0 || result.runtime.stderr.bytes !== 0) {
      fail("packaged parent emitted startup output");
    }
    ownedDescendants = mergeIdentities(ownedDescendants, descendantIdentities(childIdentity));
    result.runtime.ownedDescendantCount = ownedDescendants.length;
    if (app.quitCommand) {
      await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(app.quitCommand)})`);
      const quitExit = await waitForExitTracking(child, childIdentity, 12_000);
      ownedDescendants = mergeIdentities(ownedDescendants, quitExit.descendants ?? []);
      result.quitCommandExited = quitExit.exited;
      if (!quitExit.exited || quitExit.code !== 0 || quitExit.signal !== null) {
        fail("packaged orderly quit command did not exit cleanly");
      }
    }
    result.status = "PASS";
  } catch (error) {
    result.error = publicErrorMessage(error, "unknown packaged smoke failure");
    if (result.error.startsWith("stale packaged-acceptance")) {
      result.cleanup.releaseGateBlocked = true;
    }
  } finally {
    if (secondChild?.pid) {
      if (secondChild.exitCode === null) {
        try {
          secondIdentity ??= await waitForProcessIdentity(secondChild.pid, executable);
          ownedDescendants = mergeIdentities(ownedDescendants, descendantIdentities(secondIdentity));
          const secondCleanup = await stopOwnedProcess(secondIdentity, executable, secondChild);
          ownedDescendants = mergeIdentities(
            ownedDescendants,
            secondCleanup.descendants ?? [],
          );
          result.cleanup.secondProcess = {
            forced: secondCleanup.forced,
            alreadyExited: secondCleanup.alreadyExited,
          };
        } catch (error) {
          result.cleanup.secondProcessError = publicErrorMessage(error, "second process cleanup failed");
        }
      }
    }
    try {
      const trackedSecondIdentities = new Set(
        ownedDescendants.map((identity) => `${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
      );
      uncertainDescendants = mergeIdentities(
        uncertainDescendants,
        potentialNewDescendants(secondIdentity, processesBeforeSecond).filter(
          (identity) =>
            !trackedSecondIdentities.has(`${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
        ),
      );
    } catch {
      result.cleanup.processInventoryError = true;
    }
    try {
      if (child?.pid) {
        ownedDescendants = mergeIdentities(ownedDescendants, descendantIdentities(childIdentity));
      }
    } catch {
      result.cleanup.processInventoryError = true;
    }
    try {
      cdp?.close();
    } catch {
      result.cleanup.cdpCloseError = true;
    }
    if (child) {
      try {
        const processCleanup = await stopOwnedProcess(childIdentity, executable, child);
        ownedDescendants = mergeIdentities(
          ownedDescendants,
          processCleanup.descendants ?? [],
        );
        result.cleanup.process = {
          forced: processCleanup.forced,
          alreadyExited: processCleanup.alreadyExited,
        };
      } catch (error) {
        result.cleanup.processError = publicErrorMessage(error, "process cleanup failed");
      }
    }
    try {
      const trackedPrimaryIdentities = new Set(
        ownedDescendants.map((identity) => `${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
      );
      uncertainDescendants = mergeIdentities(
        uncertainDescendants,
        potentialNewDescendants(childIdentity, processesBeforePrimary).filter(
          (identity) =>
            !trackedPrimaryIdentities.has(`${identity.Pid}:${identity.Created}:${identity.Name}:${identity.Path}`),
        ),
      );
    } catch {
      result.cleanup.processInventoryError = true;
    }
    const dataErrors = [];
    let processCleanupConfirmed = false;
    result.cleanup.remainingImageCount = -1;
    result.cleanup.remainingOwnedDescendantsBeforeForce = -1;
    result.cleanup.remainingOwnedDescendants = -1;
    result.cleanup.remainingUncertainDescendants = -1;
    result.cleanup.uncertainDescendantCount = -1;
    try {
      await sleep(3_000);
      result.cleanup.remainingImageCount = imageProcesses(imageName).length;
      const remainingDescendants = survivingIdentities(ownedDescendants);
      result.cleanup.remainingOwnedDescendantsBeforeForce = remainingDescendants.length;
      for (const descendant of remainingDescendants) terminateIdentity(descendant, true);
      await sleep(500);
      result.cleanup.remainingOwnedDescendants = survivingIdentities(ownedDescendants).length;
      result.cleanup.remainingUncertainDescendants = survivingIdentities(uncertainDescendants).length;
      result.cleanup.uncertainDescendantCount = uncertainDescendants.length;
      result.cleanup.primaryIdentityConfirmed = !child?.pid || Boolean(childIdentity);
      result.cleanup.secondIdentityConfirmed = !secondChild?.pid || Boolean(secondIdentity);
      processCleanupConfirmed =
        !result.cleanup.processInventoryError &&
        result.cleanup.primaryIdentityConfirmed &&
        result.cleanup.secondIdentityConfirmed &&
        result.cleanup.remainingImageCount === 0 &&
        result.cleanup.remainingOwnedDescendants === 0 &&
        result.cleanup.uncertainDescendantCount === 0 &&
        result.cleanup.remainingUncertainDescendants === 0 &&
        matchingWindowsProcesses(context.protectedProcessNames).length === 0;
    } catch {
      result.cleanup.processInventoryError = true;
    }
    if (!processCleanupConfirmed) dataErrors.push("process cleanup was not independently confirmed");
    try {
      writeTransactionJournal(transactionJournal, app.id, context.runId, "restoring", protectedRecords);
    } catch {
      dataErrors.push("transaction journal could not be updated");
    }
    // Restoration is independent from deletion. Even after a process-inventory
    // failure, atomically move only our marker-owned generated directory aside
    // and restore the original backup when the filesystem permits it. Unknown
    // or still-locked data remains preserved with the transaction journal.
    for (const record of protectedRecords.reverse()) {
      if (!record.mutated) continue;
      try {
        if (pathExists(record.staging)) {
          if (!isOwnedDirectory(record.staging, context.runId)) {
            fail("app-data staging lost harness ownership");
          }
          if (processCleanupConfirmed) {
            removeGenerated(record.staging, [context.actualLocalAppData]);
          } else {
            dataErrors.push("app-data staging preserved after uncertain process cleanup");
          }
        }
        if (record.prepared && pathExists(record.directory)) {
          if (!isOwnedDirectory(record.directory, context.runId)) {
            fail("app-data path was recreated without harness ownership");
          }
          if (pathExists(record.quarantine)) fail("generated-data quarantine collision");
          renameSync(record.directory, record.quarantine);
        }
        if (record.backedUp) {
          if (pathExists(record.directory) || !pathExists(record.backup)) {
            fail("app-data restore precondition failed");
          }
          renameSync(record.backup, record.directory);
          if (!pathExists(record.directory) || pathExists(record.backup)) {
            fail("app-data restore read-back failed");
          }
        }
        if (pathExists(record.quarantine)) {
          if (!isOwnedDirectory(record.quarantine, context.runId)) {
            fail("generated-data quarantine lost harness ownership");
          }
          if (processCleanupConfirmed) {
            removeGenerated(record.quarantine, [context.actualLocalAppData]);
          } else {
            dataErrors.push("generated-data quarantine preserved after uncertain process cleanup");
          }
        }
        if (record.originalExisted && !pathExists(record.directory)) {
          fail("original app-data path was not restored");
        }
        if (!record.originalExisted && pathExists(record.directory)) {
          fail("generated app-data current path remained after cleanup");
        }
      } catch (error) {
        dataErrors.push(publicErrorMessage(error, "app-data restore failed"));
      }
    }
    try {
      if (isolatedRoot) removeGenerated(isolatedRoot, [context.runtimeRoot]);
    } catch (error) {
      dataErrors.push(publicErrorMessage(error, "isolated runtime cleanup failed"));
    }
    try {
      result.cleanup.backupResidue = protectedRecords.filter(({ backup }) => pathExists(backup)).length;
      result.cleanup.quarantineResidue = protectedRecords.filter(({ quarantine }) => pathExists(quarantine)).length;
      result.cleanup.stagingResidue = protectedRecords.filter(({ staging }) => pathExists(staging)).length;
    } catch {
      result.cleanup.backupResidue = -1;
      result.cleanup.quarantineResidue = -1;
      result.cleanup.stagingResidue = -1;
      dataErrors.push("app-data residue could not be inspected");
    }
    if (
      transactionJournal &&
      dataErrors.length === 0 &&
      result.cleanup.backupResidue === 0 &&
      result.cleanup.quarantineResidue === 0 &&
      result.cleanup.stagingResidue === 0
    ) {
      try {
        rmSync(transactionJournal);
      } catch {
        dataErrors.push("transaction journal cleanup failed");
      }
    }
    try {
      result.cleanup.journalResidue = transactionJournal && pathExists(transactionJournal) ? 1 : 0;
    } catch {
      result.cleanup.journalResidue = -1;
      dataErrors.push("transaction journal residue could not be inspected");
    }
    result.cleanup.dataRestored =
      dataErrors.length === 0 &&
      result.cleanup.backupResidue === 0 &&
      result.cleanup.quarantineResidue === 0 &&
      result.cleanup.stagingResidue === 0 &&
      result.cleanup.journalResidue === 0;
    if (dataErrors.length > 0) result.cleanup.dataErrorCount = dataErrors.length;
    if (
      result.cleanup.remainingImageCount !== 0 ||
      result.cleanup.remainingOwnedDescendants !== 0 ||
      result.cleanup.uncertainDescendantCount !== 0 ||
      result.cleanup.remainingUncertainDescendants !== 0 ||
      !processCleanupConfirmed ||
      !result.cleanup.dataRestored ||
      result.cleanup.backupResidue !== 0 ||
      result.cleanup.quarantineResidue !== 0 ||
      result.cleanup.stagingResidue !== 0 ||
      result.cleanup.journalResidue !== 0
    ) {
      result.status = "FAIL";
      result.cleanup.releaseGateBlocked = true;
    }
    if (result.cleanup.remainingOwnedDescendantsBeforeForce > 0) {
      result.status = "FAIL";
      result.cleanup.descendantLeakObserved = true;
    }
    if (result.cleanup.uncertainDescendantCount > 0) {
      result.status = "FAIL";
      result.cleanup.descendantOwnershipUncertain = true;
    }
  }
  return result;
}

async function main() {
  if (process.platform !== "win32") fail("run this harness with Windows Node");
  const [nodeMajor, nodeMinor] = process.versions.node.split(".").map(Number);
  if (nodeMajor < 22 || (nodeMajor === 22 && nodeMinor < 5)) {
    fail("Windows Node 22.5 or newer is required");
  }
  if (typeof WebSocket === "undefined") fail("Windows Node must provide WebSocket");
  const args = parseArgs(process.argv.slice(2));
  const configFile = path.resolve(args.get("config") ?? fail("missing --config"));
  const verificationFile = path.resolve(args.get("verification") ?? fail("missing --verification"));
  const assetsDirectory = path.resolve(args.get("assets") ?? fail("missing --assets"));
  const outputFile = path.resolve(args.get("output") ?? fail("missing --output"));
  const runtimeRoot = path.resolve(args.get("runtime") ?? fail("missing --runtime"));
  const expectedTag = args.get("tag") ?? fail("missing --tag");
  const expectedCommit = args.get("commit") ?? fail("missing --commit");
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(expectedTag)) fail("invalid --tag");
  if (!/^[0-9a-f]{40}$/.test(expectedCommit)) fail("invalid --commit");
  const config = JSON.parse(readFileSync(configFile, "utf8"));
  const verification = JSON.parse(readFileSync(verificationFile, "utf8"));
  const manifest = JSON.parse(readFileSync(path.join(assetsDirectory, "release-manifest.json"), "utf8"));
  const runId = `${Date.now()}-${process.pid}`;
  if (pathExists(outputFile)) fail("refusing to overwrite an acceptance result");
  const actualLocalAppData = process.env.LOCALAPPDATA;
  if (!actualLocalAppData || !path.isAbsolute(actualLocalAppData)) {
    fail("Windows LOCALAPPDATA is unavailable");
  }
  const knownLocalAppData = windowsLocalAppData();
  if (path.resolve(actualLocalAppData).toLowerCase() !== path.resolve(knownLocalAppData).toLowerCase()) {
    fail("Windows LOCALAPPDATA does not match the LocalApplicationData known folder");
  }
  assertPlainPathAndAncestors(actualLocalAppData, true);
  assertSafeConfig(config);
  assertSafeScratchLayout(
    configFile,
    verificationFile,
    assetsDirectory,
    outputFile,
    runtimeRoot,
    actualLocalAppData,
  );
  if (
    verification.status !== "PASS" ||
    verification.draft !== false ||
    verification.prerelease !== expectedTag.includes("-") ||
    verification.tag !== expectedTag ||
    verification.commit !== expectedCommit ||
    verification.releaseAssets !== 32 ||
    verification.downloadedAssets !== 32 ||
    verification.manifestApps !== 15 ||
    verification.manifestDeclaredAssets !== 31 ||
    verification.verifiedAssets !== 32 ||
    verification.configSha256 !== sha256File(configFile) ||
    verification.missing !== 0 ||
    verification.undeclared !== 0 ||
    !Array.isArray(verification.failures) ||
    verification.failures.length !== 0
  ) {
    fail("independent release-asset verification is missing or does not match this run");
  }
  const protectedProcessNames = [
    ...new Set(
      config.apps.flatMap((app) => [
        `${app.id}.exe`,
        ...(app.additionalProcessNames ?? []),
      ]),
    ),
  ];
  if (matchingWindowsProcesses(protectedProcessNames).length > 0) {
    fail("close all Devbox processes before packaged acceptance");
  }
  const protectedIdentifiers = new Set([
    "devbox",
    ...config.apps.flatMap((app) => [app.identifier, ...(app.legacyIdentifiers ?? [])]),
  ]);
  for (const identifier of protectedIdentifiers) {
    assertNoStaleAcceptancePaths(path.join(actualLocalAppData, identifier), actualLocalAppData);
  }
  if (manifest.releaseTag !== expectedTag || manifest.apps.length !== config.apps.length) {
    fail("release manifest tag or application count mismatch");
  }
  const actualTemp = process.env.TEMP ?? process.env.TMP;
  if (!actualTemp || !path.isAbsolute(actualTemp)) fail("Windows temporary directory is unavailable");
  assertPlainPathAndAncestors(actualTemp, true);
  acquireAcceptanceLock(path.join(actualTemp, ".devbox-packaged-acceptance.lock"), runId);
  mkdirSync(runtimeRoot, { recursive: true });
  const context = {
    assetsDirectory,
    runtimeRoot,
    manifest,
    runId,
    actualLocalAppData,
    sharedLocalDataRoot: path.join(actualLocalAppData, "devbox"),
    protectedProcessNames,
  };
  const report = {
    schemaVersion: 1,
    tag: expectedTag,
    expectedCommit,
    independentAssetVerificationSha256: sha256File(verificationFile),
    acceptanceConfigSha256: sha256File(configFile),
    startedAt: new Date().toISOString(),
    host: {
      platform: process.platform,
      architecture: process.arch,
      node: process.version,
    },
    apps: [],
  };
  for (const app of config.apps) {
    const result = await runApp(app, context);
    report.apps.push(result);
    writeJson(outputFile, { ...report, completedAt: null });
    if (result.cleanup?.releaseGateBlocked || requestedSignal) break;
  }
  report.completedAt = new Date().toISOString();
  report.summary = {
    passed: report.apps.filter((app) => app.status === "PASS").length,
    failed: report.apps.filter((app) => app.status === "FAIL").length,
    skipped: report.apps.filter((app) => app.status === "SKIP").length,
    unattempted: config.apps.length - report.apps.length,
    interrupted: requestedSignal,
  };
  try {
    rmdirSync(runtimeRoot);
    report.runtimeRootRemoved = true;
  } catch {
    report.runtimeRootRemoved = false;
  }
  writeJson(outputFile, report);
  if (
    report.summary.failed > 0 ||
    report.summary.skipped > 0 ||
    report.summary.unattempted > 0 ||
    !report.runtimeRootRemoved ||
    requestedSignal
  ) {
    process.exitCode = requestedSignal ? 130 : 2;
  }
}

try {
  await main();
} finally {
  releaseAcceptanceLock();
}
