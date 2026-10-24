import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

const FAKE_LSP = String.raw`#!/usr/bin/env node
let pending = Buffer.alloc(0);
function send(value) { const body = Buffer.from(JSON.stringify(value)); process.stdout.write('Content-Length: ' + body.length + '\r\n\r\n'); process.stdout.write(body); }
process.stdin.on('data', chunk => {
  pending = Buffer.concat([pending, chunk]);
  for (;;) {
    const end = pending.indexOf('\r\n\r\n'); if (end < 0) break;
    const size = Number(/Content-Length: (\d+)/i.exec(pending.subarray(0, end).toString())[1]);
    if (pending.length < end + 4 + size) break;
    const message = JSON.parse(pending.subarray(end + 4, end + 4 + size)); pending = pending.subarray(end + 4 + size);
    if (message.method === 'initialize') send({ jsonrpc: '2.0', id: message.id, result: { capabilities: { positionEncoding: 'utf-16', textDocumentSync: 1 } } });
    if (message.method === 'textDocument/didOpen') require('node:fs').writeFileSync(process.env.HOME + '/lsp-opened', '1');
    if (message.method === 'textDocument/didChange') send({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: { uri: message.params.textDocument.uri, version: message.params.textDocument.version, diagnostics: [{ range: { start: { line: 0, character: 6 }, end: { line: 0, character: 7 } }, severity: 1, message: 'owned LSP diagnostic' }] } });
  }
});
`;

test.beforeEach(async ({ stack }) => {
  writeFileSync(join(stack.home, "bin/typescript-language-server"), FAKE_LSP, { mode: 0o700 });
});

// Authored in S4-2; run with the assembled S4 branch at S4-9.
test("code saves manually and switching files keeps undo local to each file", async ({ page, stack }) => {
  const projectId = await stack.gammaProjectId();
  const folder = join(stack.home, "projects/gamma");
  writeFileSync(join(folder, "owned-a.ts"), "const a = 1;\n");
  writeFileSync(join(folder, "owned-b.ts"), "const b = 1;\n");
  await page.goto(
    `/source?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom&root=${encodeURIComponent(`project:${projectId}`)}&path=owned-a.ts`,
  );
  const editor = page.getByRole("textbox", { name: "코드 편집기" });
  await expect(editor).toHaveAttribute("aria-readonly", "false");
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("// owned A");
  await expect(page.getByRole("status", { name: "저장 상태" })).toHaveText("저장하지 않음 · Ctrl+S");
  expect(readFileSync(join(folder, "owned-a.ts"), "utf8")).toBe("const a = 1;\n");
  await page.keyboard.press("Control+s");
  await expect.poll(() => readFileSync(join(folder, "owned-a.ts"), "utf8")).toContain("// owned A");
  await page.getByRole("button", { name: "owned-b.ts", exact: true }).click();
  await expect(editor).toContainText("const b");
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("// owned B");
  await page.getByRole("tab", { name: "owned-a.ts" }).click();
  await editor.click();
  await page.keyboard.press("Control+z");
  await expect(editor).not.toContainText("// owned A");
  await page.getByRole("tab", { name: "owned-b.ts" }).click();
  await expect(editor).toContainText("// owned B");
  expect(readFileSync(join(folder, "owned-b.ts"), "utf8")).toBe("const b = 1;\n");
});

// Authored in S4-4; run only with the assembled S4 branch at S4-9.
test("owned fake LSP diagnostics appear in the editor and Problems without saving", async ({ page, stack }) => {
  const projectId = await stack.gammaProjectId();
  const file = join(stack.home, "projects/gamma/lsp-owned.ts");
  writeFileSync(file, "const a = 1;\n");
  await page.goto(
    `/source?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom&root=${encodeURIComponent(`project:${projectId}`)}&path=lsp-owned.ts`,
  );
  const editor = page.getByRole("textbox", { name: "코드 편집기" });
  await expect(editor).toHaveAttribute("aria-readonly", "false");
  await expect(page.locator("summary").filter({ hasText: "언어 서버 · 실행 중" })).toBeVisible();
  await expect.poll(() => existsSync(join(stack.home, "lsp-opened"))).toBe(true);
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("// dirty");
  await expect(page.locator(".cm-lintRange-error")).toHaveCount(1);
  if (!(await page.getByRole("tab", { name: "문제", exact: true }).isVisible())) await page.keyboard.press("Control+`");
  await page.getByRole("tab", { name: "문제", exact: true }).click();
  await expect(page.getByRole("region", { name: "실행 문제" })).toContainText("owned LSP diagnostic");
  expect(readFileSync(file, "utf8")).toBe("const a = 1;\n");
});
