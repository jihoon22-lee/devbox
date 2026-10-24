import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

// The fixture HOME is owned by the test, so the default vault `~/notes` is a fixture folder.
const vaultOf = (home: string) => {
  const vault = join(home, "notes");
  mkdirSync(vault, { recursive: true });
  return vault;
};
const notesUrl = (gateway: string, search = "") =>
  `/notes?gateway=${encodeURIComponent(gateway)}&renderer=dom${search ? `&${search}` : ""}`;

test("a new note saves itself a second after typing", async ({ page, stack }) => {
  const vault = vaultOf(stack.home);
  await page.goto(notesUrl(stack.gatewayUrl));
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.getByRole("button", { name: "새 노트", exact: true }).click();
  const editor = page.getByRole("textbox", { name: "노트 편집기" });
  await expect(editor).toHaveAttribute("aria-readonly", "false");
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("\nowned e2e line");
  const status = page.getByRole("status", { name: "저장 상태" });
  await expect(status).toHaveText(/^저장됨 \d\d:\d\d$/, { timeout: 5_000 });
  await expect.poll(() => readFileSync(join(vault, "새 노트.md"), "utf8")).toContain("owned e2e line");
});

test("an outside edit reloads a clean note and opens the comparison for a note being edited", async ({
  page,
  stack,
}) => {
  const file = join(vaultOf(stack.home), "outside.md");
  writeFileSync(file, "first\n");
  await page.goto(notesUrl(stack.gatewayUrl, "path=outside.md"));
  const editor = page.getByRole("textbox", { name: "노트 편집기" });
  await expect(editor).toContainText("first", { timeout: 10_000 });
  writeFileSync(file, "second\n");
  await expect(editor).toContainText("second", { timeout: 10_000 });
  await expect(page.getByRole("region", { name: "충돌 비교" })).toBeHidden();
  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("mine");
  writeFileSync(file, "third\n");
  await expect(page.getByRole("region", { name: "충돌 비교" })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("status", { name: "저장 상태" })).toHaveText("충돌 · 비교 필요");
  // Nothing overwrote the outside edit.
  expect(readFileSync(file, "utf8")).toBe("third\n");
});

test("renaming a note rewrites [[links]] in the notes that point to it", async ({ page, stack }) => {
  const vault = vaultOf(stack.home);
  writeFileSync(join(vault, "plan.md"), "# plan\n");
  writeFileSync(join(vault, "today.md"), "see [[plan]] and [[plan|the plan]]\n");
  await page.goto(notesUrl(stack.gatewayUrl));
  await page.getByRole("button", { name: "plan 메뉴", exact: true }).click({ timeout: 10_000 });
  await page.getByRole("menuitem", { name: "이름 바꾸기", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "노트 이름 바꾸기" });
  await dialog.getByLabel("새 이름").fill("roadmap");
  await dialog.getByRole("button", { name: "이름 바꾸기", exact: true }).click();
  await expect
    .poll(() => readFileSync(join(vault, "today.md"), "utf8"), { timeout: 10_000 })
    .toBe("see [[roadmap]] and [[roadmap|the plan]]\n");
  await expect(page.getByText("노트를 옮기고 링크를 고쳤습니다")).toBeVisible();
  expect(readFileSync(join(vault, "roadmap.md"), "utf8")).toBe("# plan\n");
});
