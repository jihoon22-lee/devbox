import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

const searchUrl = (gateway: string) => `/search?gateway=${encodeURIComponent(gateway)}&renderer=dom`;

test("Hangul partial note search opens the matching note", async ({ page, stack }) => {
  const vault = join(stack.home, "notes");
  mkdirSync(vault, { recursive: true });
  writeFileSync(join(vault, "restart.md"), "# owned note\n데몬재시작을 확인합니다.\n");
  await expect
    .poll(async () => (await stack.rpc("search.notes", { text: "재시작", limit: 20 })).items.length, { timeout: 10000 })
    .toBeGreaterThan(0);
  await page.goto(searchUrl(stack.gatewayUrl));
  await page.getByRole("textbox", { name: "검색어" }).fill("재시작");
  await page.getByRole("button", { name: "검색", exact: true }).click();
  await page.getByRole("button", { name: /owned note.*restart.md/ }).click();
  await page.getByRole("button", { name: "노트 열기" }).click();
  await expect(page.getByRole("textbox", { name: "노트 편집기" })).toContainText("데몬재시작");
});

test("a project code result opens a read-only preview", async ({ page, stack }) => {
  await stack.gammaProjectId();
  writeFileSync(join(stack.home, "projects/gamma/owned-search.ts"), "export const ownedSearchMarker = true;\n");
  await page.goto(searchUrl(stack.gatewayUrl));
  await page.getByRole("textbox", { name: "검색어" }).fill("ownedSearchMarker");
  await page.getByRole("button", { name: "검색", exact: true }).click();
  await page.getByRole("button", { name: /owned-search.ts/ }).click();
  const preview = page.getByRole("textbox", { name: "검색 결과 코드" });
  await expect(preview).toContainText("ownedSearchMarker");
  await expect(preview).toHaveAttribute("aria-readonly", "true");
});
