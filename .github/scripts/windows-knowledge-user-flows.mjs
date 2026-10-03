// Real packaged UI acceptance. Provisioning and final namespace cleanup belong to the Suite fixture.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile, writeFile, mkdir, rename, lstat, realpath } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { freePort, connect } from "./workspace-cdp-fixture.mjs";
import {
  allWindowsProcesses,
  windowsProcessIsElevated,
  inspectElevatedCdpPolicy,
  installElevatedCdpPolicy,
  releaseCdpSession,
  stopOwnedProcess,
} from "./windows-packaged-smoke.mjs";
import { focusWindowsCdpHost } from "./windows-cdp-host.mjs";
import { captureWindowOwner, nativeWindowAction } from "./windows-user-flow-window.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { packagedIdentity, writeUserFlowResults, fileDigest } from "./suite-user-flow-results.mjs";
import { typedComponentBridge } from "./typed-component-fixture.mjs";
import { createOwnedActivityWindow } from "./windows-owned-activity-window.mjs";
import { run as documents, scenarioIds as documentIds } from "./windows-knowledge-document-recovery.mjs";
import { run as search, scenarioIds as searchIds } from "./windows-knowledge-search-lifecycle.mjs";
import { run as activity, scenarioIds as activityIds } from "./windows-knowledge-activity.mjs";
export async function runInstalledKnowledgeUserFlows() {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  const identity = await packagedIdentity(),
    root = await realpath(process.env.DEVBOX_USER_FLOW_INSTALL_ROOT ?? "");
  const scratch = path.dirname(root),
    runner = path.resolve(process.env.RUNNER_TEMP);
  assert.ok(path.resolve(scratch).startsWith(runner + path.sep));
  assert.match(path.basename(scratch), /^devbox-suite-delivery-[a-f0-9]{32}$/u);
  assert.equal(path.basename(root), "Suite UI Fixture");
  const readJson = async (file) => JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/u, ""));
  const owner = await readJson(path.join(scratch, "user-flow-owner.json")),
    registration = await readJson(path.join(root, "suite-registration.json")),
    manifest = await readJson(path.join(root, "devbox-installation.json")),
    activation = await readJson(path.join(root, "devbox-activation.json"));
  assert.equal(owner.root, root);
  assert.equal(owner.sourceSha, identity.sourceSha);
  assert.match(owner.installationKey, /^[a-f0-9]{64}$/u);
  assert.equal(registration.installationKey, owner.installationKey);
  assert.equal(activation.phase, "committed");
  const payload = await readJson(path.join(root, "suite-payload.json"));
  assert.equal(payload.sourceSha, identity.sourceSha);
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, 1);
  const member = manifest.members.find((item) => item.product === "knowledge");
  assert.ok(member);
  assert.equal(member.executable, `generations/${manifest.generation}/products/knowledge/devbox-knowledge.exe`);
  const executable = await realpath(path.join(root, member.executable));
  assert.ok(executable.startsWith(root + path.sep));
  assert.equal(await fileDigest(executable), member.sha256);
  const release = await readJson(path.join(process.env.DEVBOX_USER_FLOW_ASSETS, "release-manifest.json"));
  const image = release.products.find((p) => p.id === "knowledge").files.find((f) => f.name === "devbox-knowledge.exe");
  assert.equal(member.sha256, image.sha256);
  const fixtureRoot = path.join(scratch, "knowledge-user-flow-data");
  await mkdir(fixtureRoot, { recursive: true });
  let current = null,
    offline = null;
  const profile = path.join(fixtureRoot, "webview-profile");
  await mkdir(profile);
  async function wait(check, label, timeout = 30000) {
    const until = Date.now() + timeout;
    while (Date.now() < until) {
      try {
        if (await check()) return;
      } catch {}
      await delay(100);
    }
    throw new Error(label);
  }
  async function launch() {
    const port = await freePort();
    const policy = windowsProcessIsElevated() ? inspectElevatedCdpPolicy(path.basename(executable), port) : null;
    if (policy) installElevatedCdpPolicy(policy);
    const env = {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      WEBVIEW2_USER_DATA_FOLDER: profile,
    };
    for (const key of Object.keys(env)) if (/TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY/i.test(key)) delete env[key];
    const child = spawn(executable, [], { cwd: root, env, stdio: "ignore" });
    await once(child, "spawn");
    current = { child, identity: null, executable, policy, cdp: null };
    await wait(
      () =>
        allWindowsProcesses().some(
          (row) => row.Pid === child.pid && path.resolve(row.Path).toLowerCase() === executable.toLowerCase(),
        ),
      "owned Knowledge identity",
    );
    const processIdentity = allWindowsProcesses().find((row) => row.Pid === child.pid);
    current.identity = processIdentity;
    current.windowOwner = captureWindowOwner(processIdentity, root);
    current.cdp = await connect(port, child);
    assert.equal((await focusWindowsCdpHost(processIdentity)).state, "focused");
    await wait(
      () => current.cdp.evaluate("!!window.__TAURI_INTERNALS__ && !!document.querySelector('.product-shell')"),
      "native Knowledge shell",
    );
    return current;
  }
  async function closeOwned() {
    assert.ok(current?.windowOwner);
    nativeWindowAction(current.windowOwner, "Close");
  }
  async function finish(crash = false) {
    if (!current) return;
    const item = current;
    if (item.child.exitCode === null) {
      if (crash) {
        assert.ok(item.identity, "No owned process identity; preserve failed fixture");
        await stopOwnedProcess(item.identity, executable, item.child);
      } else {
        await closeOwned();
        await wait(() => item.child.exitCode !== null, "owned Knowledge close");
      }
    }
    await releaseCdpSession(item);
    current = null;
  }
  const ui = createUiDriver({
    cdp: { command: (method, args) => current.cdp.command(method, args) },
    evidenceRoot: "product-foundation-evidence/user-flows/screenshots/knowledge",
    closeOwnedWindow: closeOwned,
  });
  const allowed = {
    "knowledge.notes": new Set(["get_root", "load_note_journal"]),
    "knowledge.activity": new Set([
      "get_idle_threshold",
      "knowledge_draft_history",
      "collection_status",
      "get_privacy_rules",
      "timeline",
    ]),
    "knowledge.search": new Set(["list_roots", "index_status"]),
    "knowledge.commands": new Set(["lifecycle_status"]),
  };
  async function observe(component, method, args = {}) {
    assert.ok(allowed[component]?.has(method), "Only read-only fixture native observations allowed");
    const route = component.includes("activity") ? "activity" : component.includes("search") ? "search" : "notes";
    const result = await current.cdp.evaluate(
      `(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;${typedComponentBridge}const d=await invoke('plugin:product-shell|describe');const header={protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+5000,route:${JSON.stringify(route)},context:d.context};return invokeComponent('knowledge',{request:{header,component:${JSON.stringify(component)},method:${JSON.stringify(method)},args:${JSON.stringify(args)}}});})()`,
    );
    assert.equal(result.operation.outcome.state, "succeeded");
    return result.value;
  }
  async function waitFor(target) {
    await wait(async () => {
      const { nodes } = await current.cdp.command("Accessibility.getFullAXTree");
      return nodes.some((n) => !n.ignored && n.role?.value === target.role && n.name?.value === target.name);
    }, `UI ${target.name}`);
  }
  const body = () => current.cdp.evaluate("document.body.innerText"),
    waitBody = (text) => wait(async () => (await body()).includes(text), `UI text ${text}`);
  let notesRoot;
  const fixture = {
    kind: "owned-installed-windows",
    ownerVerified: true,
    wait,
    body,
    waitBody,
    waitFor,
    async navigate(route) {
      const d = await current.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('plugin:product-shell|describe')");
      const feature = d.features?.find((f) => f.route === route);
      assert.ok(feature);
      await ui.click({ role: "button", name: feature.label });
    },
    async prepareNotes() {
      await wait(async () => {
        notesRoot = await observe("knowledge.notes", "get_root");
        return Boolean(notesRoot);
      }, "validated native vault ready");
      const canonical = await realpath(notesRoot);
      assert.ok(
        canonical.startsWith(scratch + path.sep) || canonical.includes(`.i${owner.installationKey}${path.sep}`),
        "Vault belongs to prepared fixture",
      );
      assert.ok(!(await lstat(canonical)).isSymbolicLink());
      const dir = path.join(canonical, "Notes");
      await mkdir(dir, { recursive: true });
      const token = randomUUID().replaceAll("-", ""),
        a = `Notes/ui-${token}-A.md`,
        b = `Notes/ui-${token}-B.md`,
        aOriginal = "# 합성 A\n",
        bOriginal = "# 합성 B\n";
      await writeFile(path.join(canonical, a), aOriginal, { flag: "wx" });
      await writeFile(path.join(canonical, b), bOriginal, { flag: "wx" });
      return { a, b, aFile: path.join(canonical, a), bFile: path.join(canonical, b), aOriginal, bOriginal };
    },
    async openNote(rel) {
      const name = path.basename(rel);
      await waitFor({ role: "button", name });
      await ui.click({ role: "button", name });
      await waitFor({ role: "textbox", name: "Markdown 본문" });
    },
    async disableAutosave() {
      const enabled = await current.cdp.evaluate(
        "document.querySelector('.note-toolbar input[type=checkbox]')?.checked ?? Array.from(document.querySelectorAll('label')).find(e=>e.textContent.includes('자동 저장'))?.querySelector('input')?.checked",
      );
      if (enabled) await ui.click({ role: "checkbox", name: "자동 저장" });
    },
    journal: () => observe("knowledge.notes", "load_note_journal"),
    async waitJournal(rel, content) {
      await wait(
        async () => (await this.journal()).entries.some((e) => e.path === rel && e.content === content),
        "native journal exact bytes",
      );
    },
    async crashAndReopen() {
      await finish(true);
      await launch();
    },
    async reopenAfterClose() {
      await wait(() => current.child.exitCode !== null, "reviewed exit");
      await finish();
      await launch();
    },
    async closeAndReopen() {
      await finish();
      await launch();
    },
    async confirmDialog() {
      await wait(async () => {
        await current.cdp.command("Page.handleJavaScriptDialog", { accept: true });
        return true;
      }, "explicit native confirmation");
    },
    async offlineAndReopen() {
      await finish(true);
      offline = `${notesRoot}.offline-${randomUUID()}`;
      await rename(notesRoot, offline);
      await launch();
    },
    async offlineContents(rel) {
      assert.ok(offline);
      return readFile(path.join(offline, rel), "utf8");
    },
    async restoreOnline() {
      await finish(true);
      await rename(offline, notesRoot);
      offline = null;
      await launch();
    },
    roots: () => observe("knowledge.search", "list_roots"),
    async waitRoot(value) {
      await wait(
        async () => (await this.roots()).some((r) => path.resolve(r.path) === path.resolve(value)),
        "native root ack",
      );
    },
    async contentStats() {
      const stats = await observe("knowledge.search", "index_status");
      return { indexed_files: stats.content_indexed_files };
    },
    regexEnabled: () => current.cdp.evaluate("document.querySelector('.regex-toggle input')?.checked===true"),
    async removeRootViaUi(value) {
      await ui.click({ role: "button", name: `${value} 루트 제거` });
      await wait(
        async () => !(await this.roots()).some((r) => path.resolve(r.path) === path.resolve(value)),
        "root removal acknowledged",
      );
    },
    agentIdentity() {
      const agents = allWindowsProcesses().filter(
        (row) =>
          path
            .resolve(row.Path ?? "")
            .toLowerCase()
            .startsWith(root.toLowerCase() + path.sep) &&
          path
            .resolve(row.Path ?? "")
            .toLowerCase()
            .endsWith(path.join("resources", "suite", "devbox-agent.exe").toLowerCase()),
      );
      assert.equal(agents.length, 1);
      return agents[0];
    },
    assertSameAgent(expected) {
      const actual = this.agentIdentity();
      assert.equal(actual.Pid, expected.Pid);
      assert.equal(actual.Created, expected.Created);
    },
    idleThreshold: () => observe("knowledge.activity", "get_idle_threshold"),
    history: () => observe("knowledge.activity", "knowledge_draft_history"),
    collectionStatus: () => observe("knowledge.activity", "collection_status"),
    privacy: () => observe("knowledge.activity", "get_privacy_rules"),
    lifecycle: () => observe("knowledge.commands", "lifecycle_status"),
    selectedDate: () => current.cdp.evaluate("document.querySelector('input[type=date]').value"),
    async createDraftViaUi() {
      await waitFor({ role: "button", name: "Knowledge로 보내기" });
      await ui.click({ role: "button", name: "Knowledge로 보내기" });
      await waitBody("Knowledge 초안");
    },
    async captureOwnedIdleBoundary() {
      const window = await createOwnedActivityWindow(path.join(fixtureRoot, `foreground-${randomUUID()}`));
      try {
        const lastInputMs = await window.input();
        await delay(4000);
        await window.input();
        const cutoff = Date.now();
        await delay(65000);
        const date = new Date(cutoff),
          start = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime(),
          end = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime();
        const sessions = (await observe("knowledge.activity", "timeline", { dayStart: start, dayEnd: end })).filter(
          (row) => row.title.startsWith(window.marker),
        );
        return { lastInputMs: cutoff, marker: window.marker, sessions };
      } finally {
        await window.close();
        await focusWindowsCdpHost(current.identity);
      }
    },
  };
  const context = { ...identity, ui, knowledgeFixture: fixture, fixtureRoot };
  let results = [];
  try {
    await launch();
    results = [...(await documents(context)), ...(await search(context)), ...(await activity(context))];
    await writeUserFlowResults("knowledge", results);
    assert.ok(
      results.every((r) => r.status === "PASS"),
      "Knowledge real user-flow acceptance failed",
    );
  } catch (error) {
    if (!results.length) {
      results = [...documentIds, ...searchIds, ...activityIds].map((id) => ({
        ...identity,
        id,
        status: "FAIL",
        evidenceKind: "packaged-ui",
        assertions: ["Owned packaged UI startup failed before scenario execution"],
        screenshotPaths: [],
        failureCode: "knowledge-owned-startup-failed",
      }));
      await writeUserFlowResults("knowledge", results);
    }
    throw error;
  } finally {
    if (offline) {
      await finish(true);
      await rename(offline, notesRoot);
      offline = null;
    }
    await finish(true);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  await runInstalledKnowledgeUserFlows();
