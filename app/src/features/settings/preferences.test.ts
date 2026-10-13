import {
  restorePreferences,
  setStartSection,
  setTerminalFont,
  setTheme,
  setUiScale,
  startSection,
  storedFontSize,
  usePreferences,
} from "./preferences";

afterEach(() => vi.unstubAllGlobals());
test("system theme follows changes and removes its listener on cleanup", () => {
  const media = new EventTarget() as EventTarget & { matches: boolean };
  media.matches = false;
  vi.stubGlobal("matchMedia", () => media);
  setTheme("system");
  const cleanup = restorePreferences();
  expect(document.documentElement.dataset.theme).toBe("light");
  media.matches = true;
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.dataset.theme).toBe("dark");
  cleanup();
  media.matches = false;
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.dataset.theme).toBe("dark");
});
test("preferences remain finite, bounded and usable when persistence is denied", () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("denied");
  });
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("denied");
  });
  setTerminalFont(99);
  expect(storedFontSize()).toBe(28);
  setUiScale(Number.NaN);
  expect(document.documentElement.style.zoom).toBe("1");
  expect(usePreferences.getState().uiScale).toBe(1);
  setUiScale(100);
  expect(usePreferences.getState().uiScale).toBe(1.5);
});
test("start section only accepts known section paths and retains last as default", () => {
  for (const invalid of ["https://example.test", "/popout/agent/owned", "/settings", ""]) {
    localStorage.setItem("devbox.startSection", invalid);
    expect(startSection()).toBe("last");
  }
  localStorage.setItem("devbox.startSection", "/agents");
  expect(startSection()).toBe("/agents");
  localStorage.removeItem("devbox.startSection");
});

test("write-only storage failure keeps the chosen font and start section over readable old values", () => {
  localStorage.setItem("devbox.terminalFont", "13");
  localStorage.setItem("devbox.startSection", "/notes");
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  setTerminalFont(22);
  setStartSection("/terminal");
  expect(usePreferences.getState().terminalFont).toBe(22);
  expect(storedFontSize()).toBe(22);
  expect(startSection()).toBe("/terminal");
});
