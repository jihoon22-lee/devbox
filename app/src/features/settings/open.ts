import { create } from "zustand";
export const SETTINGS_TABS = ["general", "agents", "secrets", "shortcuts", "diagnostics", "about"] as const;
export type SettingsTab = (typeof SETTINGS_TABS)[number];
export const useSettingsUi = create<{ tab: SettingsTab; setTab(tab: SettingsTab): void }>((set) => ({
  tab: "general",
  setTab: (tab) => set({ tab }),
}));
let navigate: (() => void) | undefined;
export function registerSettingsNavigation(callback: () => void) {
  navigate = callback;
  return () => {
    if (navigate === callback) navigate = undefined;
  };
}
export function openSettings(tab: SettingsTab) {
  useSettingsUi.getState().setTab(tab);
  navigate?.();
}
