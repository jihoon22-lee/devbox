/** The tool a project's last new task used, kept on this PC so the next one starts with it. */
const key = (projectId: string) => `devbox.newTask.lastProfile.${projectId}`;
/** Before tools were remembered per project, one key held the last tool used anywhere. It is only read. */
const LEGACY_KEY = "devbox.lastProfile";

/**
 * The remembered tool id, or "" when there is none. A project with no memory of its own starts from
 * the old global key; a memory of its own wins even when that tool is gone, so a stale one is not
 * replaced by another project's choice. Whether the tool still exists is the caller's to check.
 */
export function rememberedProfile(projectId: string | undefined) {
  if (!projectId) return "";
  try {
    return localStorage.getItem(key(projectId)) ?? localStorage.getItem(LEGACY_KEY) ?? "";
  } catch {
    return "";
  }
}

export function rememberProfile(projectId: string, profile: string) {
  try {
    localStorage.setItem(key(projectId), profile);
  } catch {
    /* Preference storage is optional. */
  }
}
