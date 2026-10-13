import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

test("launch profile opens its terminals in one action", async ({ page, stack }) => {
  const directory = join(stack.home, "projects/gamma/.devbox");
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "devbox.toml"), '[launch]\nterminals = [{ name = "web" }, { name = "logs" }]\n');
  await stack.gammaProjectId();
  await page.goto(`/agents?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "gamma 시작 구성 실행" }).click();
  await expect(page).toHaveURL(/\/terminal/);
  expect(page.url()).toContain("gateway=");
  expect(page.url()).toContain("renderer=dom");
  await expect(page.getByRole("tab", { name: "web" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "logs" })).toBeVisible();
});
