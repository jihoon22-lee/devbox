/** HTTP authorization is an explicit action owned by one live send, never an automatic redirect. */
import { openExternal } from "../../../shell/external";
import { type HttpAuthorization, ownsSend, useApiTabs } from "./store";

export function authorizationUrl(value: string): string | null {
  if (value.length > 64 * 1024 || value !== value.trim() || /[\u0000-\u0020\u007f]/.test(value)) return null;
  try {
    const url = new URL(value);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return null;
    return url.href;
  } catch {
    return null;
  }
}
function current(id: string, action: HttpAuthorization): boolean {
  const state = useApiTabs.getState();
  const tab = state.tabs.find((tab) => tab.id === id);
  return tab?.authorization === action && tab.key === state.active && ownsSend(id, action.owner);
}
export async function approveHttpAuthorization(id: string, action: HttpAuthorization): Promise<void> {
  if (!current(id, action) || action.opening) return;
  const url = authorizationUrl(action.url);
  if (!url) return;
  const opening = { ...action, opening: true, error: undefined };
  useApiTabs.getState().updateById(id, { authorization: opening });
  try {
    await openExternal(url);
    if (current(id, opening))
      useApiTabs.getState().updateById(id, { authorization: { ...opening, opening: false, opened: true } });
  } catch {
    if (current(id, opening))
      useApiTabs.getState().updateById(id, {
        authorization: { ...opening, opening: false, error: "브라우저를 열지 못했습니다. 다시 시도해 주세요." },
      });
  }
}
