import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Locator, Page } from "@playwright/test";
import { each, hex, mostCommon, near, pixelsOf, type Rgba } from "./pixels";
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
/** A mark (border, underline, inset outline) drawn on its element's fill, against the colours on both sides. */
function expectVisibleMark(mark: string, inside: Rgba, outside: Rgba, label: string) {
  // A translucent mark is seen over the fill it is painted on.
  const seen = over(parse(mark), inside);
  expect(contrast(seen, inside), `${label} against its own fill`).toBeGreaterThanOrEqual(3);
  expect(contrast(seen, outside), `${label} against the surrounding background`).toBeGreaterThanOrEqual(3);
}

/**
 * A label paints its glyphs: inside its box most pixels are the row's own fill and some stand apart from it.
 * Computed colours cannot show this. A forced-colours backplate covers the box in Canvas, which can be the
 * text colour itself (S8-5b-8 M2, RC3-SHOT-01).
 */
async function expectPaintedLabel(page: Page, label: Locator, row: Locator, name: string) {
  const fill = mostCommon(await pixelsOf(page, row));
  const box = each(await pixelsOf(page, label));
  const ink = box.filter((color) => !near(color, fill)).length / box.length;
  expect(ink, `${name}: glyph pixels apart from the row fill ${hex(fill)}`).toBeGreaterThanOrEqual(0.05);
  expect(ink, `${name}: the label box shows the row fill ${hex(fill)}, not a backplate`).toBeLessThanOrEqual(0.6);
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

/**
 * A Kbd chip in a state no shipped control shows yet (S8-5b-8 M1): a copy of `host` with `attributes` and a
 * copy of a real chip, hovered beside the original, measured and removed again.
 */
async function expectReadableChipIn(page: Page, host: Locator, attributes: Record<string, string>, label: string) {
  await host.evaluate((element, extra) => {
    const copy = element.cloneNode(true) as HTMLElement;
    for (const [name, value] of Object.entries(extra)) copy.setAttribute(name, value);
    copy.dataset.probe = "";
    const chip = document.querySelector("kbd");
    if (!chip) throw new Error("no chip to copy");
    if (!copy.querySelector("kbd")) copy.append(chip.cloneNode(true));
    element.after(copy);
  }, attributes);
  const probe = page.locator("[data-probe]");
  await probe.hover();
  expectReadable(await look(probe.locator("kbd")), label);
  await probe.evaluate((element) => element.remove());
}

test("primary-kbd-and-danger-highlight-meet-contrast", async ({ page, stack }) => {
  test.slow();
  const projectId = await stack.gammaProjectId();
  await stack.rpc("projects.set_favorite", { id: projectId, favorite: true });
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
      await expectReadableChipIn(
        page,
        primary,
        { "aria-pressed": "true" },
        "shortcut chip in a pressed primary button",
      );
      // A chip outside a primary button keeps its default colours.
      const plain = await look(page.getByRole("button", { name: "명령 팔레트 열기" }).locator("kbd"));
      expect(plain.color).toBe(await resolve(page, "--fg-muted"));
      expect(plain.border).toBe(await resolve(page, "--border"));

      await page.getByRole("button", { name: "프로젝트 전환" }).click();
      const projects = page.getByRole("dialog", { name: "프로젝트", exact: true });
      // The filled star is the favourite's state; a pressed box would sit beside the row's own marks (M4).
      const star = projects.getByRole("button", { name: "gamma 즐겨찾기", exact: true });
      await expect(star).toHaveAttribute("aria-pressed", "true");
      for (const state of ["resting", "hovered"]) {
        if (state === "hovered") await star.hover();
        const seen = await look(star);
        expect(parse(seen.background)[3], `${state} favourite star fill`).toBe(0);
        expect(parse(seen.border)[3], `${state} favourite star border`).toBe(0);
        expect(await star.locator("svg").evaluate((svg) => getComputedStyle(svg).fill), "filled star").toBe(seen.color);
      }
      await projects.getByRole("button", { name: "gamma 제거", exact: true }).click();
      const confirm = page.getByRole("dialog", { name: "프로젝트 제거", exact: true });
      const danger = confirm.getByRole("button", { name: "프로젝트 제거", exact: true });
      expectReadable(await look(danger), "danger button");
      await danger.hover();
      expectReadable(await look(danger), "hovered danger button");
      await expectReadableChipIn(page, danger, {}, "shortcut chip in a hovered danger button");
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
      const highlighted = await look(trash);
      expectReadable(highlighted, "highlighted danger menu item");
      // The danger fill carries the highlight; an accent frame on red would read as a second state.
      expect(highlighted.outline, "highlighted danger menu item outline").toBe(await resolve(page, "--danger"));
      await expectReadableChipIn(page, trash, {}, "shortcut chip in a highlighted danger menu item");
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
  // Chromium's light and dark forced palettes both have HighlightText equal to Canvas.
  for (const scheme of ["light", "dark"] as const) {
    await test.step(`${scheme} forced palette`, async () => {
      await page.emulateMedia({ forcedColors: "active", colorScheme: scheme });
      await page.goto(at("/agents", stack.gatewayUrl));
      await expect(page.getByText("데몬 연결됨")).toBeVisible();
      await page.mouse.move(0, 0);
      const [highlight, highlightText, canvas] = await Promise.all(
        ["Highlight", "HighlightText", "Canvas"].map((name) => resolve(page, name)),
      );
      const list = page.getByRole("button", { name: "목록", exact: true });
      await expect(list).toHaveAttribute("aria-pressed", "true");
      expect((await look(list)).border, "pressed button border").toBe(highlight);
      const grid = await look(page.getByRole("button", { name: "격자", exact: true }));
      expect(grid.border, "unpressed button border").not.toBe(highlight);
      await list.hover();
      expect((await look(list)).border, "hovered pressed button border").toBe(highlight);

      await page.getByRole("button", { name: "명령 팔레트 열기" }).click();
      const options = page.getByRole("dialog", { name: "명령" }).getByRole("option");
      const option = options.first();
      await expect(option).toHaveAttribute("aria-selected", "true");
      expect(await look(option), "active palette option").toMatchObject({
        background: highlight,
        outline: highlight,
        color: highlightText,
      });
      await expectPaintedLabel(page, option.locator("span").first(), option, "active palette option");
      const chip = await look(option.locator("kbd"));
      expect(chip, "shortcut chip in the active option").toMatchObject({ color: highlightText, border: highlightText });
      expectReadable(chip, "shortcut chip in the active option");
      const other = options.nth(1);
      const rest = await look(other);
      expect(rest.background, "other palette option fill").not.toBe(highlight);
      expect(rest.outlineStyle, "other palette option outline").toBe("none");
      await expectPaintedLabel(page, other.locator("span").first(), other, "other palette option");
      await page.keyboard.press("Escape");

      await page.goto(at("/source", stack.gatewayUrl));
      const tabs = page.getByRole("tablist", { name: "소스 보기" });
      const files = tabs.getByRole("tab", { name: "파일", exact: true });
      await expect(files).toHaveAttribute("aria-selected", "true");
      expect((await look(files)).underline, "selected source tab underline").toBe(highlight);
      for (const name of ["변경", "브랜치"]) {
        const unselected = await look(tabs.getByRole("tab", { name, exact: true }));
        expect(unselected.underline, `unselected [${name}] has no forced underline`).toBe(canvas);
      }
      const editor = page.getByRole("button", { name: "편집기", exact: true });
      await expect(editor).toHaveAttribute("aria-pressed", "true");
      expect((await look(editor)).border, "pressed source toggle border").toBe(highlight);
      const log = page.getByRole("button", { name: "커밋 기록", exact: true });
      await expect(log).toHaveAttribute("aria-pressed", "false");
      expect((await look(log)).border, "unpressed source toggle border").not.toBe(highlight);

      await page.goto(at("/notes", stack.gatewayUrl));
      await page.getByRole("button", { name: "owned 메뉴", exact: true }).click({ timeout: 10_000 });
      const trash = page.getByRole("menuitem", { name: "휴지통으로 옮기기", exact: true });
      await trash.hover();
      await expect(trash).toHaveAttribute("data-highlighted", "");
      expect(await look(trash), "highlighted danger menu item").toMatchObject({
        background: highlight,
        color: highlightText,
      });
      await expectPaintedLabel(page, trash.locator("span").first(), trash, "highlighted danger menu item");
      const rename = page.getByRole("menuitem", { name: "이름 바꾸기", exact: true });
      expect((await look(rename)).background, "other menu item fill").not.toBe(highlight);
      await expectPaintedLabel(page, rename.locator("span").first(), rename, "other menu item");
      await page.keyboard.press("Escape");

      await page.goto(at("/agents", stack.gatewayUrl));
      await page.getByRole("button", { name: "새 작업 Ctrl+Shift+N", exact: true }).click();
      await page.getByRole("combobox", { name: "도구", exact: true }).press("Enter");
      const listbox = page.getByRole("listbox");
      await expect(listbox).toBeVisible();
      await page.keyboard.press("ArrowDown");
      const lit = listbox.locator('[role="option"][data-highlighted]');
      await expect(lit).toHaveCount(1);
      expect(await look(lit), "highlighted Select option").toMatchObject({
        background: highlight,
        color: highlightText,
      });
      await expectPaintedLabel(page, lit.locator("span").first(), lit, "highlighted Select option");
      const unlit = listbox.locator('[role="option"]:not([data-highlighted])').first();
      expect((await look(unlit)).background, "other Select option fill").not.toBe(highlight);
      await expectPaintedLabel(page, unlit.locator("span").first(), unlit, "other Select option");
      await page.keyboard.press("Escape");
    });
  }
});

const BASIC = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
/** The run log's 16 basic foreground colours (SGR 30–37, 90–97) and their tokens. */
const ANSI16 = [
  ...BASIC.map((name, index) => ({ code: 30 + index, token: `--log-ansi-${name}` })),
  ...BASIC.map((name, index) => ({ code: 90 + index, token: `--log-ansi-bright-${name}` })),
];

test.describe("run log", () => {
  test.use({ runtimeEnabled: true });

  test("ansi16-log-contrast-survives-theme-switch", async ({ page, stack }) => {
    test.slow();
    const projectId = await stack.gammaProjectId();
    // One line in the owned fixture project: every basic foreground colour, then a white-on-red badge.
    const codes = ANSI16.map(({ code }) => code).join(" ");
    const command = String.raw`for code in ${codes}; do printf '\033[%smfg-%s\033[0m ' "$code" "$code"; done; printf '\033[37;41mbadge\033[0m\n'`;
    await stack.rpc("runtime.define", {
      projectId,
      saveTo: "thisPc",
      def: {
        kind: "task",
        id: "owned-ansi16",
        command,
        cwd: null,
        env: {},
        target: "linux",
        dependsOn: [],
        worktree: null,
        secrets: [],
      },
    });
    const run = await stack.rpc("runtime.start", { projectId, id: "owned-ansi16", worktree: null });
    await expect
      .poll(async () => (await stack.rpc("runtime.get", { runId: run.id })).run.exit?.code, { timeout: 15_000 })
      .toBe(0);
    // The run page selects the project's only definition and shows its latest run.
    await page.addInitScript(
      (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
      projectId,
    );
    const log = page.getByRole("region", { name: "실행 로그" });
    const readColours = async (label: string) => {
      const spans: { code: number; token: string; seen: Look }[] = [];
      for (const { code, token } of ANSI16)
        spans.push({ code, token, seen: await look(log.getByText(`fg-${code}`, { exact: true })) });
      const unreadable = spans.flatMap(({ code, seen }) => {
        const ratio = textContrast(seen);
        return ratio < 4.5 ? [`SGR ${code} ${seen.color}: ${ratio.toFixed(2)}`] : [];
      });
      const fill = backdrop(spans[0]!.seen.layers).slice(0, 3).map(Math.round).join(", ");
      expect(unreadable, `${label}: log colours under 4.5:1 on rgb(${fill})`).toEqual([]);
      const colours: Record<number, string> = {};
      for (const { code, token, seen } of spans) {
        expect(seen.color, `${label}: SGR ${code} is ${token}`).toBe(await resolve(page, token));
        colours[code] = seen.color;
      }
      // An undefined token leaves both the span and the probe on the inherited --fg, and passes the checks above.
      const tokens = [...ANSI16.map(({ token }) => token), "--log-ansi-bg-red"];
      const undefinedTokens = await page.evaluate(
        (names) => names.filter((name) => !getComputedStyle(document.documentElement).getPropertyValue(name).trim()),
        tokens,
      );
      expect(undefinedTokens, `${label}: undefined tokens`).toEqual([]);
      // A badge keeps its classic pair, #e5e5e5 on #cd3131, in every theme. Its contrast is not claimed.
      expect(await look(log.getByText("badge", { exact: true })), `${label}: badge`).toMatchObject({
        color: "rgb(229, 229, 229)",
        background: "rgb(205, 49, 49)",
      });
      return colours;
    };
    const byTheme: Record<string, Record<number, string>> = {};
    for (const theme of THEMES) {
      await test.step(theme.name, async () => {
        await showTheme(page, at("/runs", stack.gatewayUrl), theme);
        await expect(log.getByText("fg-97", { exact: true })).toBeVisible();
        byTheme[theme.theme] = await readColours(theme.name);
      });
    }
    // Light grey (SGR 90) sits between black (30) and white (37), apart from both (S8-5b-9 review).
    const light = byTheme.light!;
    for (const code of [30, 37]) {
      const apart = contrast(parse(light[90]!), parse(light[code]!));
      expect(apart, `light SGR 90 apart from SGR ${code}`).toBeGreaterThanOrEqual(1.3);
    }
    await test.step("system theme follows the OS to dark without a reload", async () => {
      // The last step left the system preference in a light OS. The lines already on screen must follow.
      await page.emulateMedia({ colorScheme: "dark" });
      await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
      const switched = await readColours("switched to dark");
      expect(switched).toEqual(byTheme.dark);
      expect(switched).not.toEqual(byTheme.light);
    });
  });
});
