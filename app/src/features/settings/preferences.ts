import { create } from "zustand";

export type Theme = "system" | "dark" | "light";
export const START_SECTIONS = ["last", "/agents", "/terminal", "/runs", "/source", "/notes", "/tools"] as const;
export type StartSection = (typeof START_SECTIONS)[number];
const unsaved = new Map<string, string>();
function read(key: string) {
  if (unsaved.has(key)) return unsaved.get(key)!;
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function save(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
    unsaved.delete(key);
  } catch {
    unsaved.set(key, value);
  }
}
const bounded = (value: number, fallback: number, min: number, max: number) =>
  Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
function readFontSize(fallback = 13) {
  const raw = read("devbox.terminalFont");
  return bounded(raw?.trim() ? Number(raw) : fallback, 13, 9, 28);
}
export function storedFontSize() {
  return readFontSize(usePreferences.getState().terminalFont);
}
export function startSection(): StartSection {
  const saved = read("devbox.startSection");
  return START_SECTIONS.find((item) => item === saved) ?? "last";
}
const savedTheme = read("devbox.theme");
export const usePreferences = create<{
  theme: Theme;
  uiScale: number;
  terminalFont: number;
  startSection: StartSection;
}>(() => ({
  theme: savedTheme === "light" || savedTheme === "dark" ? savedTheme : "system",
  uiScale: bounded(Number(read("devbox.uiScale") ?? 1), 1, 0.9, 1.5),
  terminalFont: readFontSize(),
  startSection: startSection(),
}));
function applyTheme() {
  const preference = usePreferences.getState().theme;
  document.documentElement.dataset.theme =
    preference === "system"
      ? window.matchMedia?.("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : preference;
}
export function setTheme(theme: Theme) {
  usePreferences.setState({ theme });
  save("devbox.theme", theme);
  applyTheme();
}
export function setUiScale(scale: number) {
  const uiScale = Math.round(bounded(scale, 1, 0.9, 1.5) * 100) / 100;
  usePreferences.setState({ uiScale });
  document.documentElement.style.zoom = String(uiScale);
  save("devbox.uiScale", String(uiScale));
}
export function restoreUiScale() {
  setUiScale(bounded(Number(read("devbox.uiScale") ?? 1), 1, 0.9, 1.5));
}
export function setTerminalFont(size: number, notify = true) {
  const terminalFont = bounded(size, 13, 9, 28);
  usePreferences.setState({ terminalFont });
  save("devbox.terminalFont", String(terminalFont));
  if (notify) window.dispatchEvent(new Event("devbox-terminal-font"));
}
export function setStartSection(section: StartSection) {
  usePreferences.setState({ startSection: section });
  save("devbox.startSection", section);
}
export function restorePreferences() {
  applyTheme();
  restoreUiScale();
  const media = window.matchMedia?.("(prefers-color-scheme: dark)");
  media?.addEventListener("change", applyTheme);
  const storage = (event: StorageEvent) => {
    if (event.key) unsaved.delete(event.key);
    if (event.key === "devbox.theme") {
      const saved = read(event.key);
      usePreferences.setState({ theme: saved === "dark" || saved === "light" ? saved : "system" });
      applyTheme();
    } else if (event.key === "devbox.uiScale") setUiScale(Number(read(event.key) ?? 1));
    else if (event.key === "devbox.terminalFont") setTerminalFont(storedFontSize());
    else if (event.key === "devbox.startSection") usePreferences.setState({ startSection: startSection() });
  };
  window.addEventListener("storage", storage);
  return () => {
    media?.removeEventListener("change", applyTheme);
    window.removeEventListener("storage", storage);
  };
}
