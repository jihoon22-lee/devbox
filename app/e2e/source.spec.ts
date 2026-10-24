import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

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
