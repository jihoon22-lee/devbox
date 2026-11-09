import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./support";

type Box = { x: number; y: number; width: number; height: number };
// The smallest main window (tauri.conf.json minWidth/minHeight); 150% leaves 640×400 CSS px.
const WINDOW = { width: 960, height: 600 };
const LONG = "customer-portal-billing-reconciliation-service-with-an-extremely-long-project-name";
test.use({ viewport: WINDOW });

const at = (path: string, gateway: string) =>
  `${path}${path.includes("?") ? "&" : "?"}gateway=${encodeURIComponent(gateway)}`;
/** Loads `url` at the UI scale the settings page stores (`setUiScale`: root zoom and `--ui-zoom`). */
async function showScale(page: Page, url: string, scale: number) {
  if (page.url() === "about:blank") await page.goto(url);
  await page.evaluate((value) => localStorage.setItem("devbox.uiScale", String(value)), scale);
  await page.goto(url);
  await expect.poll(() => page.evaluate(() => document.documentElement.style.zoom)).toBe(String(scale));
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.mouse.move(0, 0);
}
async function box(locator: Locator): Promise<Box> {
  const found = await locator.boundingBox();
  if (!found) throw new Error("no box");
  return found;
}
/** Window pixels, which zoom scales: positions compare as they look. */
function expectInside(content: Box, label: string) {
  expect(content.x, `${label}: left edge`).toBeGreaterThanOrEqual(-0.5);
  expect(content.y, `${label}: top edge`).toBeGreaterThanOrEqual(-0.5);
  expect(content.x + content.width, `${label}: right edge`).toBeLessThanOrEqual(WINDOW.width + 0.5);
  expect(content.y + content.height, `${label}: bottom edge`).toBeLessThanOrEqual(WINDOW.height + 0.5);
}
/** Opened under (or, flipped, over) its trigger within 8 px, and inside the window. */
function expectBeside(content: Box, trigger: Box, label: string) {
  const below = content.y >= trigger.y + trigger.height - 1;
  const gap = below ? content.y - (trigger.y + trigger.height) : trigger.y - (content.y + content.height);
  expect(gap, `${label}: gap to its trigger`).toBeGreaterThanOrEqual(-1);
  expect(gap, `${label}: gap to its trigger`).toBeLessThanOrEqual(8);
  expectInside(content, label);
}

test("poppers-open-at-their-trigger-and-selects-keep-one-line-at-each-ui-scale", async ({ page, stack }) => {
  test.slow();
  const taskId = await stack.waitingAgent();
  // A long project name, and enough projects that the project list is taller than the window.
  const ids: string[] = [];
  for (const name of [LONG, ...Array.from({ length: 14 }, (_, index) => `scale-${index + 10}`)]) {
    mkdirSync(join(stack.home, "projects", name), { recursive: true });
    ids.push((await stack.rpc("projects.add", { path: join(stack.home, "projects", name) })).id);
  }
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    ids[0],
  );
  for (const scale of [1, 1.25, 1.5]) {
    await test.step(`UI scale ${scale * 100}%`, async () => {
      await showScale(page, at(`/agents?id=${taskId}`, stack.gatewayUrl), scale);
      // Menu.tsx aligns the menu's end with its trigger's. Triggers are measured first: an open menu or
      // list hides the rest of the page from role queries.
      const more = page
        .getByRole("article", { name: "에이전트 작업 상세" })
        .getByRole("button", { name: "더 보기", exact: true });
      const trigger = await box(more);
      await more.click();
      const menu = page.getByRole("menu");
      await expect(menu).toBeVisible();
      const content = await box(menu);
      expectBeside(content, trigger, "[더 보기] menu");
      const end = content.x + content.width - (trigger.x + trigger.width);
      expect(Math.abs(end), "[더 보기] menu end edge").toBeLessThanOrEqual(1);
      await page.keyboard.press("Escape");
      await expect(menu).toBeHidden();

      // Tooltip.tsx: to the right of its activity bar button, 6 px away, centred on it.
      const section = page.getByRole("navigation", { name: "섹션" }).getByRole("button", { name: "에이전트" });
      await section.hover();
      const tip = page.locator("[data-radix-popper-content-wrapper] > *");
      await expect(tip).toBeVisible();
      const [button, bubble] = [await box(section), await box(tip)];
      const gap = bubble.x - (button.x + button.width);
      expect(gap, "tooltip gap").toBeGreaterThanOrEqual(0);
      expect(gap, "tooltip gap").toBeLessThanOrEqual(8);
      const middle = bubble.y + bubble.height / 2 - (button.y + button.height / 2);
      expect(Math.abs(middle), "tooltip centre").toBeLessThanOrEqual(1);
      expectInside(bubble, "tooltip");
      await page.keyboard.press("Escape");
      await expect(tip).toBeHidden();

      // The list, with its [새 작업], is collapsed at this width.
      await page.keyboard.press("Control+Shift+N");
      const dialog = page.getByRole("dialog", { name: "새 에이전트 작업", exact: true });
      // A long chosen name stays on the trigger's one line, cut with an ellipsis, whole in the title.
      const project = dialog.getByRole("combobox", { name: "프로젝트", exact: true });
      await expect(project).toHaveAttribute("title", LONG);
      const value = project.locator("span").first();
      const [field, text] = [await box(project), await box(value)];
      expect(text.y, "value top").toBeGreaterThanOrEqual(field.y);
      expect(text.y + text.height, "value bottom").toBeLessThanOrEqual(field.y + field.height);
      expect(
        await value.evaluate((node) => ({
          overflow: getComputedStyle(node).textOverflow,
          wrap: getComputedStyle(node).whiteSpace,
          cut: node.scrollWidth > node.clientWidth,
        })),
        "value is one cut line",
      ).toEqual({ overflow: "ellipsis", wrap: "nowrap", cut: true });
      const list = page.getByRole("listbox");
      await project.press("Enter");
      await expect(list).toBeVisible();
      expectBeside(await box(list), field, "[프로젝트] list");
      await page.keyboard.press("Escape");
      await expect(list).toBeHidden();
      // Short options: the list starts at the trigger's start edge and takes the trigger's width.
      const tool = dialog.getByRole("combobox", { name: "도구", exact: true });
      const toolField = await box(tool);
      await tool.press("Enter");
      await expect(list).toBeVisible();
      const toolList = await box(list);
      expectBeside(toolList, toolField, "[도구] list");
      expect(Math.abs(toolList.x - toolField.x), "[도구] list start edge").toBeLessThanOrEqual(1);
      expect(Math.abs(toolList.width - toolField.width), "[도구] list width").toBeLessThanOrEqual(1);
      await page.keyboard.press("Escape");
      await expect(list).toBeHidden();
    });
  }
});
