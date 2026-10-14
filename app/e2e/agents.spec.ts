import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "./support";

test("new task → permission waiting → instruct → idle → diff → squash closes", async ({ page, stack }, info) => {
  await page.goto(`/agents?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가", exact: true }).click();
  await page.getByRole("dialog", { name: "프로젝트 추가", exact: true }).getByRole("button", { name: /gamma/ }).click();
  await expect(page.getByRole("dialog", { name: "프로젝트 추가", exact: true })).toBeHidden();
  await expect(page.getByRole("dialog", { name: "프로젝트", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "프로젝트", exact: true })).toBeHidden();
  await page.keyboard.press("Control+Shift+N");
  await page.getByLabel("제목", { exact: true }).fill("Fix login");
  await page.getByRole("combobox", { name: "도구", exact: true }).click();
  await page.getByRole("option", { name: "Fake", exact: true }).click();
  await page.getByLabel("지시문", { exact: true }).fill("fix it");
  const createdAt = Date.now();
  await page.getByRole("button", { name: "시작", exact: true }).click();
  const group = page.getByRole("region", { name: "입력 대기", exact: true });
  const taskButton = group.getByRole("button").filter({ has: page.getByText("Fix login", { exact: true }) });
  await expect(taskButton).toBeVisible({ timeout: 10_000 });
  await expect(group.getByText("needs Bash", { exact: true })).toBeVisible({ timeout: 2000 });
  const listAt = Date.now();
  const id = new URL(page.url()).searchParams.get("id");
  expect(id).toBeTruthy();
  // The marker is written immediately before the real CLI Notification hook, so this
  // includes CLI/socket delivery and is stricter than daemon-receipt → list latency.
  const hookAt = Number(BigInt(readFileSync(join(stack.home, `agent-hook-${id}.ns`), "utf8").trim()) / 1_000_000n);
  expect(Number.isFinite(hookAt)).toBe(true);
  const hookToListMs = listAt - hookAt;
  await info.attach("SC3-fixture-latency", {
    body: JSON.stringify({
      createToListMs: listAt - createdAt,
      hookInvocationToListMs: hookToListMs,
      criterionMs: 2000,
      realWindowsHookAcceptance: "deferred Task 24",
    }),
    contentType: "application/json",
  });
  expect(listAt - createdAt).toBeLessThan(10_000);
  expect(hookToListMs).toBeGreaterThanOrEqual(0);
  expect(hookToListMs).toBeLessThan(2000);
  await expect(page.getByRole("button", { name: "알림 1개", exact: true })).toBeVisible();
  await taskButton.click();
  await page.getByLabel("에이전트에게 보낼 지시").fill("yes");
  await page.keyboard.press("Control+Enter");
  await expect(
    page.getByRole("article", { name: "에이전트 작업 상세" }).getByText("다음 지시 대기", { exact: true }),
  ).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "변경", exact: true }).click();
  await expect(page.getByRole("tabpanel", { name: "변경" })).toContainText("agent.txt");
  await page.getByRole("button", { name: "병합", exact: true }).click();
  await page.getByRole("menuitem", { name: "커밋 하나로 합쳐 병합", exact: true }).click();
  await page.getByRole("dialog", { name: "병합 커밋 메시지" }).getByRole("button", { name: "내용 확인" }).click();
  const confirmation = page.getByRole("dialog", { name: "main에 병합 · Fix login", exact: true });
  await expect(confirmation).toContainText("HEAD");
  await confirmation.getByRole("button", { name: "main에 병합", exact: true }).click();
  await expect(
    page
      .getByRole("region", { name: "닫힘(24시간)", exact: true })
      .getByRole("button")
      .filter({ has: page.getByText("Fix login", { exact: true }) }),
  ).toBeVisible({ timeout: 10_000 });
  await expect.poll(async () => (await stack.rpc("agents.get", { id: id as string })).closedHow).toBe("merged");
});

test("Ctrl+Shift+J selects its independently prepared waiting task", async ({ page, stack }) => {
  const id = await stack.waitingAgent();
  await page.goto(`/terminal?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByRole("button", { name: "에이전트 1(입력 대기 1)" })).toBeVisible();
  await page.keyboard.press("Control+Shift+J");
  await expect(page).toHaveURL(new RegExp(`/agents\\?.*id=${id}`));
  await expect(page.getByRole("article", { name: "에이전트 작업 상세" })).toContainText("Waiting one");
});

test("grid previews select detail and the independent popout grid streams read-only tiles", async ({ page, stack }) => {
  await stack.waitingAgent();
  await page.goto(`/agents?view=grid&gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  const tile = page.getByRole("button", { name: "Waiting one 터미널 미리보기", exact: true });
  await expect(tile).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("region", { name: "에이전트 터미널 격자" }).locator(".xterm-rows")).toBeVisible();
  const popupPromise = page.waitForEvent("popup");
  await page
    .getByRole("region", { name: "에이전트 터미널 격자" })
    .locator("..")
    .getByRole("button", { name: "새 창으로", exact: true })
    .click();
  const popup = await popupPromise;
  await popup.waitForURL("**/popout/agents-grid?*");
  expect(popup.url()).toContain("renderer=dom");
  await expect(popup.getByRole("button", { name: "Waiting one 터미널 미리보기" })).toBeVisible();
  await expect(popup.locator(".xterm-rows")).toBeVisible();
  await expect(popup.getByRole("button", { name: /알림 \d+개/ })).toHaveCount(0);
  await popup.close();
  await tile.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/view=list/);
  expect(page.url()).toContain("gateway=");
  expect(page.url()).toContain("renderer=dom");
});
