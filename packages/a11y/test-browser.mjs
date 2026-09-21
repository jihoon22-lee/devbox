// Actual Chromium keyboard navigation, separate from jsdom selector fixtures.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { Cdp } from "../../.github/scripts/windows-packaged-smoke.mjs";
const root = path.dirname(fileURLToPath(import.meta.url));
const chrome = [process.env.DEVBOX_TEST_CHROME, "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser", process.env.PROGRAMFILES && path.join(process.env.PROGRAMFILES, "Google/Chrome/Application/chrome.exe")].find(value => value && existsSync(value));
assert.ok(chrome, "A Chromium executable is required for the actual Tab-order acceptance fixture");
const profile = mkdtempSync(path.join(tmpdir(), "devbox-a11y-browser-"));
const child = spawn(chrome, ["--headless=new", "--disable-gpu", "--disable-background-networking", "--no-first-run", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
let stderr = "", cdp;
child.stderr.on("data", chunk => { stderr = (stderr + chunk).slice(-4000); });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
try {
  const deadline = Date.now() + 15000;
  const endpointFile = path.join(profile, "DevToolsActivePort");
  while (!existsSync(endpointFile)) {
    assert.ok(Date.now() < deadline && child.exitCode === null, `Chromium did not start: ${stderr}`);
    await sleep(50);
  }
  const port = Number(readFileSync(endpointFile, "utf8").split("\n")[0]);
  let target;
  while (!target) {
    assert.ok(Date.now() < deadline, "Chromium page did not appear");
    target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(page => page.type === "page");
    if (!target) await sleep(50);
  }
  cdp = new Cdp(target.webSocketDebuggerUrl); await cdp.connect();
  const source = ts.transpileModule(readFileSync(path.join(root, "src/index.ts"), "utf8"), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  await cdp.evaluate(`(async () => {
    window.helpers = await import(${JSON.stringify("data:text/javascript;base64," + Buffer.from(source).toString("base64"))});
    document.body.innerHTML = '<style>.gone { display:none }.invisible { visibility:hidden }</style><section id="dialog"><button id="zero">zero</button><button id="second" tabindex="2">second</button><button id="first" tabindex="1">first</button><button id="negative" tabindex="-1">negative</button><div class="gone"><button>hidden ancestor</button></div><button class="invisible">hidden style</button><input id="unchecked" type="radio" name="choice"><input id="checked" type="radio" name="choice" checked><button id="last">last</button></section>';
    document.body.tabIndex=-1; document.body.focus();
  })()`);
  const press = async (shift = false) => {
    for (const type of ["keyDown", "keyUp"]) await cdp.send("Input.dispatchKeyEvent", { type, key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9, modifiers: shift ? 1 : 0 });
    return cdp.evaluate("document.activeElement.id");
  };
  const expected = ["first", "second", "zero", "checked", "last"];
  assert.deepEqual(await cdp.evaluate("helpers.tabbableElements(document.getElementById('dialog')).map(node => node.id)"), expected);
  for (const id of expected) assert.equal(await press(), id, `native Tab should reach ${id}`);
  await cdp.evaluate("document.getElementById('dialog').addEventListener('keydown', event => helpers.trapDialogKeyDown(event, event.currentTarget))");
  assert.equal(await press(), "first", "forward trap wraps in real Tab order");
  assert.equal(await press(true), "last", "reverse trap wraps in real Tab order");
  assert.equal(cdp.runtimeExceptions, 0);
  console.log("Chromium CSS visibility and actual Tab/Shift+Tab order: PASS");
} finally {
  cdp?.close();
  if (child.exitCode === null) {
    child.kill("SIGTERM");
    await Promise.race([new Promise(resolve => child.once("exit", resolve)), sleep(3000)]);
    if (child.exitCode === null) child.kill("SIGKILL");
  }
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
