import type { Locator } from "@playwright/test";
import { expect, test } from "./support";

async function focusLiveTerminal(terminal: Locator) {
  await expect(terminal.locator(".xterm-rows")).toBeVisible();
  // A mounted pane can still cover the renderer while terminal.attach is pending.
  await expect(terminal.getByRole("status")).toHaveCount(0);
  await terminal.click();
  await expect(terminal.locator(".xterm-helper-textarea")).toBeFocused();
}

test("terminal reload and daemon restart preserve scrollback and search", async ({ page, stack }) => {
  await page.goto(`/terminal?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.getByRole("button", { name: "새 터미널", exact: true }).click();
  const terminal = page.locator("[data-terminal-page] [data-keyscope=terminal]").first();
  await focusLiveTerminal(terminal);
  await page.keyboard.type("for i in $(seq 1 300); do printf 'history-%04d\\n' \"$i\"; done\n");
  await expect(terminal.locator(".xterm-rows")).toContainText("history-0300");
  await page.reload();
  await expect(terminal.locator(".xterm-rows")).toContainText("history-0300");
  // First redraw clears the viewport. History must already be in scrollback,
  // including the rows immediately before the newly painted live screen.
  const visibleRows = await terminal.locator(".xterm-rows > div").count();
  expect(visibleRows).toBeGreaterThan(5);
  expect(visibleRows).toBeLessThan(100);
  const historyMarker = `history-${String(300 - Math.floor(visibleRows * 1.5)).padStart(4, "0")}`;
  await focusLiveTerminal(terminal);
  await page.keyboard.press("Control+Shift+F");
  await page.getByRole("textbox", { name: "터미널 검색" }).fill(historyMarker);
  await page.keyboard.press("Enter");
  await expect(terminal.locator(".xterm-rows")).toContainText(historyMarker);
  await page.keyboard.press("Escape");
  await stack.stopDaemon();
  await expect(terminal).toContainText("입력 전송 안 됨");
  await terminal.click();
  await page.keyboard.type("discarded-while-offline");
  await stack.startDaemon();
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await expect(terminal.locator(".xterm-rows")).toContainText("history-0300");
  await focusLiveTerminal(terminal);
  await page.keyboard.type("echo restart-ready\n");
  await expect(terminal.locator(".xterm-rows")).toContainText("restart-ready");
  await expect(terminal.locator(".xterm-rows")).not.toContainText("discarded-while-offline");
  await page.keyboard.press("Control+Shift+F");
  await page.getByRole("textbox", { name: "터미널 검색" }).fill("history-0300");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("textbox", { name: "터미널 검색" })).toBeVisible();
});

test("split panes persist and closing a pane keeps its session available", async ({ page, stack }) => {
  await page.goto(`/terminal?gateway=${encodeURIComponent(stack.gatewayUrl)}&renderer=dom`);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.getByRole("button", { name: "새 터미널", exact: true }).click();
  const panes = page.locator("[data-terminal-page] [data-keyscope=terminal]");
  await expect(panes).toHaveCount(1);
  await focusLiveTerminal(panes.first());
  await page.keyboard.press("Control+Shift+E");
  await expect(panes).toHaveCount(2);
  await focusLiveTerminal(panes.nth(1));
  await page.keyboard.type("echo second-pane\n");
  await expect(panes.nth(1).locator(".xterm-rows")).toContainText("second-pane");
  const secondId = await page
    .locator("[data-terminal-page] [data-terminal-session]")
    .nth(1)
    .getAttribute("data-terminal-session");
  await page.reload();
  await expect(panes).toHaveCount(2);
  await expect(panes.nth(1).locator(".xterm-rows")).toContainText("second-pane");
  await focusLiveTerminal(panes.nth(1));
  await page.keyboard.press("Control+Shift+W");
  await expect(panes).toHaveCount(1);
  // Open the persisted session through its list entry; pane closure does not end it.
  const menu = page.locator(`[data-terminal-list-session="${secondId}"]`).getByRole("button", { name: / 메뉴$/ });
  await menu.click();
  const popupPromise = page.waitForEvent("popup");
  await page.getByRole("menuitem", { name: "새 창으로" }).click();
  const popup = await popupPromise;
  await popup.waitForURL(`**/popout/terminal/${secondId}?*`);
  expect(popup.url()).toContain(`/popout/terminal/${secondId}`);
  await expect(popup.locator(".xterm-rows")).toContainText("second-pane");
  expect(popup.url()).toContain("renderer=dom");
  await popup.close();
});
