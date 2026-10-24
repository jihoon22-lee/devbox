import { execFileSync, spawnSync } from "node:child_process";
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
test("code preserves undo, saves, stages one hunk and commits the selected change", async ({ page, stack }) => {
  const projectId = await stack.gammaProjectId();
  const folder = join(stack.home, "projects/gamma");
  writeFileSync(join(folder, "owned-a.ts"), "const a = 1;\n");
  writeFileSync(join(folder, "owned-b.ts"), "const b = 1;\n");
  const git = (...args: string[]) =>
    execFileSync("git", ["-C", folder, ...args], { env: stack.env, encoding: "utf8", timeout: 5000 });
  git("add", "owned-a.ts", "owned-b.ts");
  git("commit", "-qm", "seed source files");
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
  await page.getByRole("tab", { name: "owned-a.ts" }).click();
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("// staged hunk");
  await page.keyboard.press("Control+s");
  await expect.poll(() => readFileSync(join(folder, "owned-a.ts"), "utf8")).toContain("// staged hunk");
  await page.getByRole("tab", { name: "변경", exact: true }).click();
  await page
    .getByRole("region", { name: "스테이징하지 않음", exact: true })
    .getByRole("button", { name: "owned-a.ts", exact: true })
    .click();
  await page.getByRole("button", { name: "변경 묶음 1 스테이징", exact: true }).click();
  await expect.poll(() => git("diff", "--cached", "--name-only").trim()).toBe("owned-a.ts");
  await page.getByRole("textbox", { name: "커밋 메시지" }).fill("feat: edit owned source");
  await page.getByRole("button", { name: "커밋", exact: true }).click();
  await expect.poll(() => git("log", "-1", "--format=%s").trim()).toBe("feat: edit owned source");
  expect(git("show", "HEAD:owned-b.ts")).toBe("const b = 1;\n");
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

// Authored in S4-8; execution is reserved for the one S4-9 full check.
test("conflict rejects a stale preview, preserves the draft and commits a freshly confirmed resolution", async ({
  page,
  stack,
}) => {
  const projectId = await stack.gammaProjectId(),
    folder = join(stack.home, "projects/gamma");
  const git = (...args: string[]) =>
    execFileSync("git", ["-C", folder, ...args], { env: stack.env, encoding: "utf8", timeout: 5000 });
  git("switch", "-c", "owned-conflict");
  writeFileSync(join(folder, "initial.txt"), "theirs\n");
  git("commit", "-am", "theirs");
  git("switch", "main");
  writeFileSync(join(folder, "initial.txt"), "ours\n");
  git("commit", "-am", "ours");
  const merge = spawnSync("git", ["-C", folder, "merge", "owned-conflict"], {
    env: stack.env,
    encoding: "utf8",
    timeout: 5000,
  });
  expect(merge.status).toBe(1);
  await page.goto(
    `/source?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom&root=${encodeURIComponent(`project:${projectId}`)}&path=initial.txt`,
  );
  await page.getByRole("tab", { name: "변경", exact: true }).click();
  await page
    .getByRole("region", { name: "충돌", exact: true })
    .getByRole("button", { name: "initial.txt", exact: true })
    .click();
  const resolution = page.getByRole("textbox", { name: "해결안", exact: true });
  await resolution.fill("chosen resolution\n");
  const unmerged = git("ls-files", "-u");
  writeFileSync(join(folder, "initial.txt"), "external change\n");
  await page.getByRole("button", { name: "해결안 적용", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "파일 또는 Git 상태가 바뀌어" })).toBeVisible();
  expect(git("ls-files", "-u")).toBe(unmerged);
  await expect(resolution).toContainText("chosen resolution");
  await page.getByRole("button", { name: "현재 파일 다시 읽기 · 해결안 유지" }).click();
  await expect(page.getByRole("button", { name: "해결안 적용", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "해결안 적용", exact: true }).click();
  await expect.poll(() => git("ls-files", "-u")).toBe("");
  expect(readFileSync(join(folder, "initial.txt"), "utf8")).toBe("chosen resolution\n");
  await page.getByRole("textbox", { name: "커밋 메시지" }).fill("fix: resolve owned conflict");
  await page.getByRole("button", { name: "커밋", exact: true }).click();
  await expect.poll(() => git("log", "-1", "--format=%s").trim()).toBe("fix: resolve owned conflict");
});
test("source creates a local branch and switches back to main", async ({ page, stack }) => {
  await page.setViewportSize({ width: 540, height: 900 });
  const projectId = await stack.gammaProjectId(),
    folder = join(stack.home, "projects/gamma");
  const branch = () =>
    execFileSync("git", ["-C", folder, "branch", "--show-current"], {
      env: stack.env,
      encoding: "utf8",
      timeout: 5000,
    }).trim();
  await page.goto(
    `/source?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom&root=${encodeURIComponent(`project:${projectId}`)}&path=initial.txt`,
  );
  await page.getByRole("tab", { name: "파일", exact: true }).focus();
  await page.keyboard.press("End");
  await expect(page.getByRole("tab", { name: "브랜치", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("textbox", { name: "새 브랜치 이름" }).fill("owned-ui-branch");
  await page.getByRole("button", { name: "만들고 전환" }).click();
  await expect.poll(branch).toBe("owned-ui-branch");
  await page.getByRole("button", { name: "main 전환", exact: true }).click();
  await expect.poll(branch).toBe("main");
});
