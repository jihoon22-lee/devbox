import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

test("launch requires visible unchanged command consent before opening terminals", async ({ page, stack }) => {
  const directory = join(stack.home, "projects/gamma/.devbox");
  mkdirSync(directory, { recursive: true });
  const config = (command: string) =>
    `[launch]\nterminals = [{ name = "web", command = "${command}" }, { name = "logs" }]\n`;
  writeFileSync(join(directory, "devbox.toml"), config("printf approved; exec bash"));
  const project = await stack.gammaProjectId();
  await page.goto(`/agents?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "gamma 시작 구성 실행" }).click();
  const trust = page.getByRole("dialog", { name: "프로젝트 명령 실행 승인", exact: true });
  await expect(trust).toContainText("printf approved; exec bash");
  await trust.getByRole("button", { name: "승인 취소", exact: true }).click();
  expect((await stack.rpc("terminal.list", { projectId: project })).items).toHaveLength(0);
  await page.getByRole("button", { name: "gamma 시작 구성 실행" }).click();
  await expect(trust).toContainText("printf approved; exec bash");
  writeFileSync(join(directory, "devbox.toml"), config("printf changed; exec bash"));
  await trust.getByRole("button", { name: "명령 실행 승인", exact: true }).click();
  await expect(trust).toContainText("printf changed; exec bash");
  expect((await stack.rpc("terminal.list", { projectId: project })).items).toHaveLength(0);
  await trust.getByRole("button", { name: "명령 실행 승인", exact: true }).click();
  await expect(page).toHaveURL(/\/terminal/);
  expect(page.url()).toContain("gateway=");
  expect(page.url()).toContain("renderer=dom");
  await expect(page.getByRole("tab", { name: "web" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "logs" })).toBeVisible();
});
