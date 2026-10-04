import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const chrome = "/usr/bin/google-chrome";
test("Files toolbars retain complete controls at the 720px window's 110% viewport", {
  skip: !existsSync(chrome) && "isolated Chromium fixture requires local Chrome",
}, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "devbox-files-layout-"));
  try {
    const css = (
      await readFile(new URL("../../packages/workspace-features/src/files/App.css", import.meta.url), "utf8")
    ).replace(/^@import.*;$/gm, "");
    const html = `<meta charset="utf-8"><style>body{margin:0;width:655px;height:436px}${css}</style>
      <div class="workspace-feature-files"><div class="app-shell">
      <header class="app-header"><div class="app-heading"><p class="eyebrow">WORKSPACE</p><h1>파일</h1></div>
      <div class="file-toolbar"><input class="path-input" aria-label="열 파일 경로">
      ${["파일 열기", "파일 선택", "WSL 다시 연결", "작업 폴더", "빠른 열기", "저장"].map((name) => `<button class="toolbar-button">${name}</button>`).join("")}
      </div></header><div class="editor-toolbar"><button class="toolbar-button">−</button><span class="zoom-label">100%</span><button class="toolbar-button">+</button><span class="toolbar-hint">Ctrl+S 저장 · Ctrl+P 빠른 열기 · Ctrl+F 찾기</span></div>
      </div></div><pre id="result"></pre><script>
      const root=document.querySelector('.workspace-feature-files');
      result.textContent=JSON.stringify({viewport:innerWidth,width:root.getBoundingClientRect().width,scrollWidth:root.scrollWidth,
        controls:[...root.querySelectorAll('button,input')].map(el=>{const r=el.getBoundingClientRect();return {left:r.left,right:r.right,height:r.height,text:el.textContent}})});
      </script>`;
    const file = path.join(directory, "layout.html");
    await writeFile(file, html);
    const dom = execFileSync(
      chrome,
      [
        "--headless=new",
        "--no-sandbox",
        "--window-size=655,523",
        `--user-data-dir=${directory}/profile`,
        "--dump-dom",
        `file://${file}`,
      ],
      { timeout: 15_000, maxBuffer: 1024 * 1024, stdio: ["ignore", "pipe", "ignore"] },
    ).toString();
    const measurement = JSON.parse(dom.match(/<pre id="result">(.*?)<\/pre>/s)[1]);
    assert.equal(measurement.viewport, 655);
    assert.equal(measurement.width, 655);
    assert.ok(measurement.scrollWidth <= 655, JSON.stringify(measurement));
    for (const control of measurement.controls) {
      assert.ok(control.left >= 0 && control.right <= 655, JSON.stringify(control));
      assert.ok(control.height <= 32, `Control label wrapped: ${JSON.stringify(control)}`);
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
