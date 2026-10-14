import { openSettings, registerSettingsNavigation, useSettingsUi } from "./open";

test("settings navigation selects the requested tab and stale cleanup preserves a replacement owner", () => {
  const old = vi.fn(),
    current = vi.fn();
  const disposeOld = registerSettingsNavigation(old);
  const disposeCurrent = registerSettingsNavigation(current);
  disposeOld();
  openSettings("agents");
  expect(useSettingsUi.getState().tab).toBe("agents");
  expect(current).toHaveBeenCalledOnce();
  expect(old).not.toHaveBeenCalled();
  disposeCurrent();
  openSettings("general");
  expect(current).toHaveBeenCalledOnce();
});
