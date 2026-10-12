import { existsSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

test("scan, persist across reload, and remove only the registration", async ({ page, stack }) => {
  await page.goto(`/?gateway=${encodeURIComponent(stack.gatewayUrl)}`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가", exact: true }).click();
  await page.getByRole("dialog", { name: "프로젝트 추가", exact: true }).getByRole("button", { name: /alpha/ }).click();
  await expect(page.getByRole("dialog", { name: "프로젝트 추가", exact: true })).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "프로젝트 전환" })).toHaveText(/alpha/);
  await page.reload();
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  const path = join(stack.home, "projects/alpha");
  await expect(page.getByText(path, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "alpha 제거", exact: true }).click();
  await page.getByRole("button", { name: "프로젝트 제거", exact: true }).click();
  await expect(page.getByText(path, { exact: true })).toHaveCount(0);
  expect(existsSync(path)).toBe(true);
  await page.reload();
  await page.keyboard.press("Control+Shift+O");
  await expect(page.getByText("프로젝트가 없습니다. 프로젝트 추가로 폴더를 고르세요.")).toBeVisible();
});

test("manual path reports typed Korean errors and accepts an absolute directory", async ({ page, stack }) => {
  await page.goto("/");
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가", exact: true }).click();
  await page.getByLabel("폴더 경로").fill("relative/path");
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await expect(page.getByText("경로를 열 수 없습니다: relative/path", { exact: true })).toBeVisible();
  await page.getByLabel("폴더 경로").fill(join(stack.home, "projects/beta"));
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "프로젝트 추가", exact: true })).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "프로젝트 전환" })).toHaveText(/beta/);
});
