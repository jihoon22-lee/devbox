import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Locator, Page } from "@playwright/test";
import { near, pixelsOf, type Rgba } from "./pixels";
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

test("grid-tile-focus-ring-shows-on-all-four-edges", async ({ page, stack }) => {
  test.slow();
  await stack.waitingAgent();
  await page.goto(at("/agents?view=grid&renderer=dom", stack.gatewayUrl));
  const grid = page.getByRole("region", { name: "에이전트 터미널 격자" });
  const tile = grid.getByRole("button", { name: "Waiting one 터미널 미리보기", exact: true });
  await expect(tile).toBeVisible({ timeout: 10_000 });
  await grid.locator("..").getByRole("button", { name: "새 창으로", exact: true }).focus();
  await page.keyboard.press("Tab");
  await expect(tile).toBeFocused();
  expect(await tile.evaluate((node) => node.matches(":focus-visible")), "keyboard focus").toBe(true);
  const accent = await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--accent)";
    document.body.append(probe);
    const [r, g, b] = (getComputedStyle(probe).color.match(/[\d.]+/g) ?? []).map(Number);
    probe.remove();
    return [r, g, b, 1] as Rgba;
  });
  // The 2 px ring lies just inside the tile. Each edge is read away from the rounded corners, and the
  // sides once beside the 46 px caption and once beside the terminal.
  const ring = await pixelsOf(page, tile);
  const [w, h] = [ring.width, ring.height];
  const edges: Record<string, [number, number][]> = {
    "top edge, over the caption": [0, 1, 2].map((d) => [Math.round(w / 2), d]),
    "bottom edge": [1, 2, 3].map((d) => [Math.round(w / 2), h - d]),
    "left edge beside the caption": [0, 1, 2].map((d) => [d, 20]),
    "right edge beside the caption": [1, 2, 3].map((d) => [w - d, 20]),
    "left edge beside the terminal": [0, 1, 2].map((d) => [d, Math.round(h * 0.7)]),
    "right edge beside the terminal": [1, 2, 3].map((d) => [w - d, Math.round(h * 0.7)]),
  };
  for (const [edge, points] of Object.entries(edges))
    expect(
      points.some(([x, y]) => near(ring.at(x, y), accent, 40)),
      `${edge}: ${points.map(([x, y]) => ring.at(x, y).slice(0, 3).join(",")).join(" | ")}`,
    ).toBe(true);
});

test("status-bar-keeps-the-daemon-version-whole-at-640-css-px", async ({ page, stack }) => {
  test.slow();
  const projectId = await stack.gammaProjectId();
  // The owned fixture repository gets a long branch, so the Git chip has to shrink.
  const branch = "feature/rc3-status-bar-overflow-check-with-a-really-long-branch-name-for-the-git-chip";
  execFileSync("git", ["-C", join(stack.home, "projects/gamma"), "checkout", "-qb", branch], {
    env: stack.env,
    stdio: "pipe",
    timeout: 5000,
  });
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    projectId,
  );
  const footer = page.getByRole("contentinfo");
  // 960×600 at 125% and 150%: 768 and 640 CSS px wide. Below about 700 the WSL memory cell gives way.
  for (const [scale, memory] of [
    [1.25, true],
    [1.5, false],
  ] as const) {
    await test.step(`${WINDOW.width / scale} CSS px`, async () => {
      await showScale(page, at("/agents", stack.gatewayUrl), scale);
      await expect(footer.getByRole("button", { name: /^Git feature\// })).toBeVisible();
      const wsl = footer.getByRole("button", { name: /^WSL / });
      if (memory) await expect(wsl).toBeVisible();
      else await expect(wsl).toBeHidden();
      const version = footer.getByText(/^데몬 \d/);
      const [bar, cell] = [await box(footer), await box(version)];
      expect(cell.x + cell.width, "daemon version ends inside the bar").toBeLessThanOrEqual(bar.x + bar.width + 0.5);
      expect(
        await version.evaluate((node) => ({
          shrink: getComputedStyle(node).flexShrink,
          whole: node.scrollWidth <= node.clientWidth,
        })),
        "daemon version",
      ).toEqual({ shrink: "0", whole: true });
      expect(await footer.evaluate((node) => node.scrollWidth <= node.clientWidth), "bar fits its width").toBe(true);
    });
  }
});
