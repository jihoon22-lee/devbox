import assert from "node:assert/strict";
import test from "node:test";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Cdp } from "./windows-packaged-smoke.mjs";
import { createUiDriver } from "./suite-user-flow-driver.mjs";

const chrome = "/usr/bin/google-chrome";
test("real confirmation input preserves cancellation, focus and bounded layout with async native confirm", {
  skip: !existsSync(chrome) && "isolated browser fixture requires local Chrome",
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-confirm-browser-"));
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const require = createRequire(path.join(root, "apps/devbox-knowledge/package.json"));
  const { build } = createRequire(require.resolve("vite"))("esbuild");
  let browser, cdp;
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
      { stdio: "ignore" },
    );
    let port;
    for (let i = 0; i < 100; i++) {
      try {
        port = (await readFile(path.join(directory, "profile/DevToolsActivePort"), "utf8")).split("\n")[0];
        break;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
    }
    assert.ok(port, "Owned browser debugging endpoint unavailable");
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
    await cdp.send("Page.navigate", { url: pathToFileURL(html).href });
    const ui = createUiDriver({
      cdp: { command: (method, params) => cdp.send(method, params) },
      evidenceRoot: directory,
      closeOwnedWindow: async () => {},
    });
    const origin = { role: "button", name: "합성 삭제" };
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
    await ui.confirmAction(false);
    assert.equal(await cdp.evaluate("window.mutations"), 0);
    assert.equal(await cdp.evaluate("document.activeElement.id"), "origin");
    await ui.clickWithConfirmation(origin, true);
    assert.equal(await cdp.evaluate("window.mutations"), 1);
    await ui.click(origin);
    await ui.waitForTarget({ role: "dialog", name: "작업 확인" });
    await ui.press("Escape");
    assert.equal(await cdp.evaluate("window.mutations"), 1);
    assert.deepEqual(await cdp.evaluate("window.decisions"), [false, true, false]);
    assert.equal(await cdp.evaluate("window.nativeCalls"), 0);
    assert.equal(await cdp.evaluate("document.activeElement.id"), "origin");
  } finally {
    cdp?.ws?.close();
    if (browser && browser.exitCode === null) {
      const exited = new Promise((resolve) => browser.once("exit", resolve));
      browser.kill();
      await exited;
    }
    await rm(directory, { recursive: true, force: true });
  }
});
