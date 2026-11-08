import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./support";

// Both themes through the stored app preference. Each explicit preference runs against the opposite
// OS scheme, so an ignored preference fails the data-theme check. The light theme runs once more as
// "system" with a light OS scheme (S8-5b review focus 4).
const THEMES = [
  { name: "dark", preference: "dark", scheme: "light", theme: "dark" },
  { name: "light", preference: "light", scheme: "dark", theme: "light" },
  { name: "system in a light OS", preference: "system", scheme: "light", theme: "light" },
] as const;
type Theme = (typeof THEMES)[number];
type Rgba = [number, number, number, number];

/** Computed colours: `rgb()`/`rgba()`, and `color(srgb …)` for color-mix values. */
function parse(value: string): Rgba {
  const rgb = /^rgba?\(([\d.]+), ([\d.]+), ([\d.]+)(?:, ([\d.]+))?\)$/.exec(value);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), Number(rgb[4] ?? 1)];
  const srgb = /^color\(srgb ([-\d.e]+) ([-\d.e]+) ([-\d.e]+)(?: \/ ([\d.]+))?\)$/.exec(value);
  if (srgb) return [Number(srgb[1]) * 255, Number(srgb[2]) * 255, Number(srgb[3]) * 255, Number(srgb[4] ?? 1)];
  throw new Error(`unsupported computed colour ${value}`);
}
function over(top: Rgba, under: Rgba): Rgba {
  const a = top[3];
  return [top[0] * a + under[0] * (1 - a), top[1] * a + under[1] * (1 - a), top[2] * a + under[2] * (1 - a), 1];
}
/** The colour seen behind an element: its own and its ancestors' backgrounds down to the first opaque one. */
function backdrop(layers: string[]): Rgba {
  const colors = layers.map(parse);
  const base = colors.findIndex((color) => color[3] >= 1);
  if (base < 0) throw new Error(`no opaque background under ${layers.join(" < ")}`);
  return colors.slice(0, base).reduceRight((under, layer) => over(layer, under), colors[base]!);
}
/** WCAG 2.x relative luminance and contrast ratio. */
function luminance([r, g, b]: Rgba) {
  const [R, G, B] = [r, g, b].map((value) => {
    const c = value / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * R! + 0.7152 * G! + 0.0722 * B!;
}
function contrast(a: Rgba, b: Rgba) {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (high! + 0.05) / (low! + 0.05);
}

function look(locator: Locator) {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    const layers: string[] = [];
    for (let node: Element | null = element; node; node = node.parentElement)
      layers.push(getComputedStyle(node).backgroundColor);
    return {
      color: style.color,
      background: style.backgroundColor,
      border: style.borderTopColor,
      borderWidth: style.borderTopWidth,
      underline: style.borderBottomColor,
      underlineStyle: style.borderBottomStyle,
      underlineWidth: style.borderBottomWidth,
      outline: style.outlineColor,
      outlineStyle: style.outlineStyle,
      outlineWidth: style.outlineWidth,
      boxShadow: style.boxShadow,
      layers,
    };
  });
}
type Look = Awaited<ReturnType<typeof look>>;
/** A token or system colour as the browser computes it, for exact comparison with computed styles. */
function resolve(page: Page, color: string) {
  return page.evaluate((value) => {
    const probe = document.createElement("span");
    probe.style.color = value.startsWith("--") ? `var(${value})` : value;
    document.body.append(probe);
    const computed = getComputedStyle(probe).color;
    probe.remove();
    return computed;
  }, color);
}
function textContrast(seen: Look) {
  const fill = backdrop(seen.layers);
  return contrast(over(parse(seen.color), fill), fill);
}
function expectReadable(seen: Look, label: string) {
  expect(textContrast(seen), `${label}: ${seen.color} on ${seen.layers.join(" < ")}`).toBeGreaterThanOrEqual(4.5);
}
/** A mark (border, underline, outline) against the colours on both of its sides. */
function expectVisibleMark(mark: string, inside: Rgba, outside: Rgba, label: string) {
  expect(contrast(parse(mark), inside), `${label} against its own fill`).toBeGreaterThanOrEqual(3);
  expect(contrast(parse(mark), outside), `${label} against the surrounding background`).toBeGreaterThanOrEqual(3);
}

async function showTheme(page: Page, url: string, theme: Theme) {
  await page.emulateMedia({ colorScheme: theme.scheme });
  if (page.url() === "about:blank") await page.goto(url);
  await page.evaluate((preference) => localStorage.setItem("devbox.theme", preference), theme.preference);
  await page.goto(url);
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme.theme);
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  // Resting readings: the pointer may still sit where the previous pass left it.
  await page.mouse.move(0, 0);
}
const at = (path: string, gateway: string) => `${path}?gateway=${encodeURIComponent(gateway)}`;

test("pressed-selection-survives-hover-and-blur", async ({ page, stack }) => {
  test.slow();
  for (const theme of THEMES) {
    await test.step(theme.name, async () => {
      await showTheme(page, at("/agents", stack.gatewayUrl), theme);
      const list = page.getByRole("button", { name: "목록", exact: true });
      const grid = page.getByRole("button", { name: "격자", exact: true });
      await expect(list).toHaveAttribute("aria-pressed", "true");
      await expect(grid).toHaveAttribute("aria-pressed", "false");
      const [accent, selection, border] = await Promise.all(
        ["--accent", "--selection", "--border"].map((name) => resolve(page, name)),
      );
      const expectPressed = (seen: Look, label: string) => {
        expect(seen.border, `${label} border`).toBe(accent);
        expect(seen.background, `${label} fill`).toBe(selection);
        expectVisibleMark(seen.border, backdrop(seen.layers), backdrop(seen.layers.slice(1)), `${label} border`);
        expectReadable(seen, label);
      };
      expectPressed(await look(list), "pressed [목록]");
      expect((await look(grid)).border).toBe(border);
      // The activity bar IconButton gets the same single 1px border and fill, with no extra outline or shadow.
      const section = page.getByRole("navigation", { name: "섹션" }).getByRole("button", { name: "에이전트" });
      await expect(section).toHaveAttribute("aria-pressed", "true");
      const icon = await look(section);
      expectPressed(icon, "pressed activity bar icon");
      expect(icon).toMatchObject({ borderWidth: "1px", outlineStyle: "none", boxShadow: "none" });
      // The generic hover fill (--bg-3) must not replace the pressed fill.
      await list.hover();
      expectPressed(await look(list), "hovered pressed [목록]");
      await grid.click();
      await expect(grid).toHaveAttribute("aria-pressed", "true");
      await expect(list).toHaveAttribute("aria-pressed", "false");
      // The keyboard focus ring (a box-shadow) is the same on a pressed and an unpressed button.
      await page.keyboard.press("Shift+Tab");
      await expect(list).toBeFocused();
      const ring = (await look(list)).boxShadow;
      expect(ring).not.toBe("none");
      await page.keyboard.press("Tab");
      await expect(grid).toBeFocused();
      expect((await look(grid)).boxShadow).toBe(ring);
      // With focus and pointer gone, only the chosen view keeps the mark.
      await grid.blur();
      await page.mouse.move(0, 0);
      expectPressed(await look(grid), "pressed [격자] after blur");
      const unpressed = await look(list);
      expect(unpressed.border).toBe(border);
      expect(unpressed.border).not.toBe(accent);
    });
  }
});

test("source-view-selection-persists-after-focus-leaves", async ({ page, stack }) => {
  test.slow();
  const projectId = await stack.gammaProjectId();
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    projectId,
  );
  for (const theme of THEMES) {
    await test.step(theme.name, async () => {
      await showTheme(page, at("/source", stack.gatewayUrl), theme);
      const views = page.getByRole("tablist", { name: "소스 보기" });
      const files = views.getByRole("tab", { name: "파일", exact: true });
      const changes = views.getByRole("tab", { name: "변경", exact: true });
      await expect(files).toHaveAttribute("aria-selected", "true");
      await changes.click();
      await expect(changes).toHaveAttribute("aria-selected", "true");
      await page.keyboard.press("Tab");
      await expect(page.locator("#source-panel")).toBeFocused();
      await page.mouse.move(0, 0);
      const [accent, bg0, fg, muted] = await Promise.all(
        ["--accent", "--bg-0", "--fg", "--fg-muted"].map((name) => resolve(page, name)),
      );
      const selected = await look(changes);
      expect(selected).toMatchObject({
        underline: accent,
        underlineStyle: "solid",
        underlineWidth: "2px",
        background: bg0,
        color: fg,
      });
      expectVisibleMark(
        selected.underline,
        backdrop(selected.layers),
        backdrop(selected.layers.slice(1)),
        "selected tab underline",
      );
      expectReadable(selected, "selected tab");
      const other = await look(files);
      expect(parse(other.underline)[3], "unselected underline is transparent").toBe(0);
      expect(other.color).toBe(muted);
      expectReadable(other, "unselected tab");
    });
  }
});

test("palette_active_option_has_visible_outline_in_each_theme", async ({ page, stack }) => {
  test.slow();
  for (const theme of THEMES) {
    await test.step(theme.name, async () => {
      await showTheme(page, at("/agents", stack.gatewayUrl), theme);
      await page.getByRole("button", { name: "명령 팔레트 열기" }).click();
      const palette = page.getByRole("dialog", { name: "명령" });
      const options = palette.getByRole("option");
      await expect(options.first()).toHaveAttribute("aria-selected", "true");
      const accent = await resolve(page, "--accent");
      const expectOutlined = (seen: Look, label: string) => {
        expect(seen, label).toMatchObject({ outlineStyle: "solid", outlineWidth: "2px", outline: accent });
        // The outline is inset (offset -2px): it sits on the option's own fill and borders the list.
        expectVisibleMark(seen.outline, backdrop(seen.layers), backdrop(seen.layers.slice(1)), `${label} outline`);
        expectReadable(seen, label);
      };
      expectOutlined(await look(options.first()), "first active option");
      await page.getByRole("combobox", { name: "명령 검색" }).press("ArrowDown");
      await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
      expectOutlined(await look(options.nth(1)), "second active option");
      expect((await look(options.first())).outlineStyle).toBe("none");
      await page.keyboard.press("Escape");
      await expect(palette).toBeHidden();
    });
  }
});

test("primary-kbd-and-danger-highlight-meet-contrast", async ({ page, stack }) => {
  test.slow();
  await stack.gammaProjectId();
  // The fixture HOME is owned by the test, so the default vault `~/notes` is a fixture folder.
  mkdirSync(join(stack.home, "notes"), { recursive: true });
  writeFileSync(join(stack.home, "notes/owned.md"), "# owned\n");
  for (const theme of THEMES) {
    await test.step(theme.name, async () => {
      await showTheme(page, at("/agents", stack.gatewayUrl), theme);
      const primary = page.getByRole("button", { name: "새 작업 Ctrl+Shift+N", exact: true });
      const chip = primary.locator("kbd");
      expectReadable(await look(chip), "shortcut chip in [새 작업]");
      await primary.hover();
      expectReadable(await look(primary), "hovered [새 작업]");
      expectReadable(await look(chip), "shortcut chip in hovered [새 작업]");
      // A chip outside a primary button keeps its default colours.
      const plain = await look(page.getByRole("button", { name: "명령 팔레트 열기" }).locator("kbd"));
      expect(plain.color).toBe(await resolve(page, "--fg-muted"));
      expect(plain.border).toBe(await resolve(page, "--border"));

      await page.getByRole("button", { name: "프로젝트 전환" }).click();
      const projects = page.getByRole("dialog", { name: "프로젝트", exact: true });
      await projects.getByRole("button", { name: "gamma 제거", exact: true }).click();
      const confirm = page.getByRole("dialog", { name: "프로젝트 제거", exact: true });
      const danger = confirm.getByRole("button", { name: "프로젝트 제거", exact: true });
      expectReadable(await look(danger), "danger button");
      await danger.hover();
      expectReadable(await look(danger), "hovered danger button");
      await confirm.getByRole("button", { name: "취소", exact: true }).click();
      await expect(confirm).toBeHidden();
      await page.keyboard.press("Escape");
      await expect(projects).toBeHidden();

      await page.goto(at("/notes", stack.gatewayUrl));
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme.theme);
      await page.getByRole("button", { name: "owned 메뉴", exact: true }).click({ timeout: 10_000 });
      const trash = page.getByRole("menuitem", { name: "휴지통으로 옮기기", exact: true });
      await trash.hover();
      await expect(trash).toHaveAttribute("data-highlighted", "");
      expectReadable(await look(trash), "highlighted danger menu item");
      await page.keyboard.press("Escape");
      await expect(trash).toBeHidden();
    });
  }
});

test("forced-colors-keep-highlight-on-pressed-selected-and-highlighted-items", async ({ page, stack }) => {
  test.slow();
  const projectId = await stack.gammaProjectId();
  mkdirSync(join(stack.home, "notes"), { recursive: true });
  writeFileSync(join(stack.home, "notes/owned.md"), "# owned\n");
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    projectId,
  );
  await page.emulateMedia({ forcedColors: "active" });
  await page.goto(at("/agents", stack.gatewayUrl));
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  const highlight = await resolve(page, "Highlight");
  const list = page.getByRole("button", { name: "목록", exact: true });
  await expect(list).toHaveAttribute("aria-pressed", "true");
  expect((await look(list)).border, "pressed button border").toBe(highlight);
  await list.hover();
  expect((await look(list)).border, "hovered pressed button border").toBe(highlight);

  await page.getByRole("button", { name: "명령 팔레트 열기" }).click();
  const option = page.getByRole("dialog", { name: "명령" }).getByRole("option").first();
  await expect(option).toHaveAttribute("aria-selected", "true");
  expect(await look(option), "active palette option").toMatchObject({ background: highlight, outline: highlight });
  await page.keyboard.press("Escape");

  await page.goto(at("/source", stack.gatewayUrl));
  const files = page.getByRole("tablist", { name: "소스 보기" }).getByRole("tab", { name: "파일", exact: true });
  await expect(files).toHaveAttribute("aria-selected", "true");
  expect((await look(files)).underline, "selected source tab underline").toBe(highlight);

  await page.goto(at("/notes", stack.gatewayUrl));
  await page.getByRole("button", { name: "owned 메뉴", exact: true }).click({ timeout: 10_000 });
  const trash = page.getByRole("menuitem", { name: "휴지통으로 옮기기", exact: true });
  await trash.hover();
  await expect(trash).toHaveAttribute("data-highlighted", "");
  expect(await look(trash), "highlighted danger menu item").toMatchObject({ background: highlight });
  await page.keyboard.press("Escape");
});
