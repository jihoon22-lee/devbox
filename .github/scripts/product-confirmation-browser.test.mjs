import assert from "node:assert/strict";
import test from "node:test";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Cdp } from "./windows-packaged-smoke.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";
import { prepareTerminalStart } from "./windows-workspace-ui-fixture.mjs";
import { visibleControlBounds } from "./visible-control-bounds.mjs";

const chrome = "/usr/bin/google-chrome";

const pause = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
async function bounded(operation, milliseconds, message) {
  let timer;
  try {
    return await Promise.race([
      operation,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
async function activeOwnedGroup(group) {
  if (!Number.isSafeInteger(group) || group <= 1) return [];
  const processes = await readdir("/proc");
  const active = [];
  await Promise.all(
    processes
      .filter((entry) => /^\d+$/.test(entry))
      .map(async (entry) => {
        try {
          const stat = await readFile(`/proc/${entry}/stat`, "utf8");
          const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
          // Zombies cannot write the owned profile; waiting for their unrelated
          // reaper would not improve cleanup and could exceed the fixture bound.
          if (Number(fields[2]) === group && fields[0] !== "Z" && fields[0] !== "X") active.push(Number(entry));
        } catch (error) {
          if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error;
        }
      }),
  );
  return active;
}
async function waitOwnedGroup(group, milliseconds) {
  const deadline = performance.now() + milliseconds;
  do {
    if ((await activeOwnedGroup(group)).length === 0) return true;
    await pause(50);
  } while (performance.now() < deadline);
  return false;
}
async function cleanupOwnedBrowser(browser, cdp, closed, directory) {
  try {
    if (cdp && browser?.exitCode === null) {
      // Closing the browser also closes its CDP socket, so disconnection is an
      // expected shutdown response. The process observations below decide exit.
      await bounded(cdp.send("Browser.close"), 1500, "Owned Browser.close timeout").catch(() => {});
    }
  } finally {
    cdp?.close();
  }
  if (browser?.pid) {
    const group = browser.pid; // spawn(detached:true) creates only this fixture's Linux group.
    let stopped = await waitOwnedGroup(group, 2000);
    for (const signal of ["SIGTERM", "SIGKILL"]) {
      if (stopped) break;
      try {
        process.kill(-group, signal);
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
      stopped = await waitOwnedGroup(group, 2000);
    }
    if (!stopped) throw new Error("Owned browser group did not stop before cleanup deadline");
    await bounded(closed, 1500, "Owned browser close event timeout");
  }
  await rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
async function preserveFailure(primary, cleanup) {
  try {
    await cleanup();
  } catch (error) {
    console.error(
      JSON.stringify({
        fixture: "product-confirmation",
        stage: "cleanup",
        error: String(error.message).slice(0, 1000),
      }),
    );
    if (!primary) throw error;
  }
  if (primary) throw primary;
}

test("fixture cleanup cannot mask the first assertion failure", async () => {
  const primary = new Error("synthetic first assertion");
  await assert.rejects(
    preserveFailure(primary, async () => {
      throw new Error("synthetic cleanup failure");
    }),
    (error) => error === primary,
  );
});
test("fixture cleanup remains a failure when assertions passed", async () => {
  const cleanup = new Error("synthetic cleanup-only failure");
  await assert.rejects(
    preserveFailure(null, async () => {
      throw cleanup;
    }),
    (error) => error === cleanup,
  );
});

test("real confirmation input preserves cancellation, focus and bounded layout with async native confirm", {
  skip: !existsSync(chrome) && "isolated browser fixture requires local Chrome",
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-confirm-browser-"));
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const require = createRequire(path.join(root, "apps/devbox-knowledge/package.json"));
  const { build } = createRequire(require.resolve("vite"))("esbuild");
  let browser, cdp, browserClosed, firstError;
  let stage = "build";
  try {
    await build({
      stdin: {
        contents: `import React from 'react'; import {createRoot} from 'react-dom/client';
          import {confirmAction,ConfirmationHost} from './src/confirm'; import './src/styles.css';
          window.nativeCalls=0; window.confirm=async()=>{window.nativeCalls++;return false};
          window.mutations=0; window.decisions=[];
          function Fixture(){return <><button id="origin" onClick={async()=>{
            const accepted=await confirmAction('합성 데이터를 삭제할까요? 취소하면 그대로 유지됩니다.');
            window.decisions.push(accepted); if(accepted)window.mutations++;
          }}>합성 삭제</button><ConfirmationHost/></>};
          createRoot(document.getElementById('root')).render(<Fixture/>);`,
        loader: "tsx",
        resolveDir: path.join(root, "packages/product-shell"),
      },
      bundle: true,
      outfile: path.join(directory, "fixture.js"),
      define: { "process.env.NODE_ENV": '"production"' },
    });
    const html = path.join(directory, "fixture.html");
    await writeFile(
      html,
      '<meta charset="utf-8"><link rel="stylesheet" href="fixture.css"><div id="root"></div><script src="fixture.js"></script>',
    );
    stage = "browser-startup";
    browser = spawn(
      chrome,
      [
        "--headless=new",
        "--no-sandbox",
        "--disable-gpu",
        "--remote-debugging-port=0",
        `--user-data-dir=${directory}/profile`,
        "about:blank",
      ],
      { stdio: "ignore", detached: true },
    );
    browserClosed = new Promise((resolve) => browser.once("close", resolve));
    let spawnError;
    browser.once("error", (error) => {
      spawnError = error;
    });
    let port;
    // A fresh hosted browser took over eight seconds in the paired CSS probe.
    // Bound startup independently of UI readiness and stop on an early exit.
    const deadline = performance.now() + 20000;
    while (performance.now() < deadline) {
      if (spawnError) throw spawnError;
      if (browser.exitCode !== null) throw new Error(`Owned browser exited before readiness: ${browser.exitCode}`);
      try {
        port = (await readFile(path.join(directory, "profile/DevToolsActivePort"), "utf8")).split("\n")[0];
        break;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
    }
    assert.ok(port, "Owned browser debugging endpoint unavailable");
    stage = "cdp-connect";
    const targets = await (await fetch(`http://localhost:${port}/json`)).json();
    cdp = new Cdp(targets.find((target) => target.type === "page").webSocketDebuggerUrl);
    await cdp.connect();
    for (const domain of ["Page", "DOM", "Accessibility"]) await cdp.send(`${domain}.enable`);
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: 655,
      height: 436,
      deviceScaleFactor: 1,
      mobile: false,
    });
    stage = "navigate";
    await cdp.send("Page.navigate", { url: pathToFileURL(html).href });
    const ui = createUiDriver({
      cdp: { command: (method, params) => cdp.send(method, params) },
      evidenceRoot: directory,
      closeOwnedWindow: async () => {},
    });
    const origin = { role: "button", name: "합성 삭제" };
    stage = "initial-review";
    await ui.waitForTarget(origin);
    await ui.click(origin);
    await ui.waitForTarget({ role: "dialog", name: "작업 확인" });
    assert.equal(await cdp.evaluate("window.mutations"), 0);
    assert.equal(await cdp.evaluate("document.activeElement.textContent"), "취소");
    assert.equal(
      await cdp.evaluate(
        "(()=>{const r=document.querySelector('dialog').getBoundingClientRect();return r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight})()",
      ),
      true,
    );
    stage = "cancel-review";
    await ui.confirmAction(false);
    assert.equal(await cdp.evaluate("window.mutations"), 0);
    assert.equal(await cdp.evaluate("document.activeElement.id"), "origin");
    stage = "accept-review";
    await ui.clickWithConfirmation(origin, true);
    assert.equal(await cdp.evaluate("window.mutations"), 1);
    await ui.click(origin);
    await ui.waitForTarget({ role: "dialog", name: "작업 확인" });
    stage = "escape-review";
    await ui.press("Escape");
    assert.equal(await cdp.evaluate("window.mutations"), 1);
    assert.deepEqual(await cdp.evaluate("window.decisions"), [false, true, false]);
    assert.equal(await cdp.evaluate("window.nativeCalls"), 0);
    assert.equal(await cdp.evaluate("document.activeElement.id"), "origin");
    stage = "fixed-dialog-source-scrollport";
    const { frameTree } = await cdp.send("Page.getFrameTree");
    const knowledgeCss = await readFile(
      path.join(root, "packages/api-studio-features/src/knowledge/knowledge.css"),
      "utf8",
    );
    await cdp.send("Page.setDocumentContent", {
      frameId: frameTree.frame.id,
      html: `<style>${knowledgeCss}
        *{box-sizing:border-box} body{margin:0} #source{position:relative;margin-top:350px;height:60px;overflow:auto}
        .studio-knowledge-dialog{background:white} button{height:32px}
        </style><div id="source"><div class="studio-knowledge-backdrop"><div class="studio-knowledge-dialog">
        <h2>Knowledge 초안 보관</h2><p>합성 결과를 보관합니다.</p><pre>synthetic</pre>
        <button onclick="this.dataset.accepted='yes'">마스킹 사본 보관</button></div></div></div>`,
    });
    const target = { role: "button", name: "마스킹 사본 보관" };
    await ui.waitForTarget(target);
    assert.equal(
      await cdp.evaluate(`(()=>{const b=document.querySelector('button'),r=b.getBoundingClientRect();
      return document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2)===b})()`),
      true,
    );
    await ui.click(target);
    assert.equal(await cdp.evaluate("document.querySelector('button').dataset.accepted"), "yes");
    stage = "fixed-containing-block-clipping";
    for (const effect of ["transform:translateZ(0)", "filter:blur(0px)", "contain:layout", "contain:paint"]) {
      await cdp.send("Page.setDocumentContent", {
        frameId: frameTree.frame.id,
        html: `<style>body{margin:0} #source{position:relative;margin-top:100px;width:300px;height:50px;overflow:clip;${effect}}
          #fixed{position:fixed;top:0;left:0} button{position:relative;top:100px;width:120px;height:30px}</style>
          <div id="source"><div id="fixed"><button>잘린 확인</button></div></div>`,
      });
      const observed = await cdp.evaluate(`(()=>{const b=document.querySelector('button'),r=b.getBoundingClientRect();
        return {hit:document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2)===b,
          bounds:(${visibleControlBounds}).call(b,[r.left,r.top,r.right,r.bottom])}})()`);
      assert.equal(observed.hit, false, effect);
      assert.ok(observed.bounds[1] >= observed.bounds[3], effect);
      await assert.rejects(ui.click({ role: "button", name: "잘린 확인" }), /clipped outside its scrollport/);
    }
    stage = "terminal-datalist-input";
    await cdp.send("Page.setDocumentContent", {
      frameId: frameTree.frame.id,
      html: `<meta charset="utf-8"><input aria-label="시작 경로" list="cwd-recent"><datalist id="cwd-recent"></datalist>
        <input aria-label="시작 명령"><button onclick="document.getElementById('review').hidden=false">+ 터미널</button>
        <div id="review" hidden><button onclick="this.dataset.accepted='yes'">실행</button></div>`,
    });
    const { nodes } = await cdp.send("Accessibility.getFullAXTree");
    assert.equal(nodes.find((node) => !node.ignored && node.name?.value === "시작 경로")?.role?.value, "combobox");
    await prepareTerminalStart(ui, "/owned/root", "echo synthetic");
    assert.deepEqual(await cdp.evaluate("[...document.querySelectorAll('input')].map(input=>input.value)"), [
      "/owned/root",
      "echo synthetic",
    ]);
    assert.equal(await cdp.evaluate("document.querySelector('#review button').dataset.accepted"), "yes");
    console.log(JSON.stringify({ fixture: "product-confirmation", stage: "assertions-complete", status: "PASS" }));
  } catch (error) {
    firstError = error;
    console.error(
      JSON.stringify({
        fixture: "product-confirmation",
        stage,
        error: String(error.message).slice(0, 1000),
        stack: String(error.stack).slice(0, 4000),
      }),
    );
  } finally {
    await preserveFailure(firstError, () => cleanupOwnedBrowser(browser, cdp, browserClosed, directory));
  }
});
