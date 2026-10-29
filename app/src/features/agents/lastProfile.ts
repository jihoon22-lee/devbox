/** The tool a project's last new task used, kept on this PC so the next one starts with it. */
const key = (projectId: string) => `devbox.newTask.lastProfile.${projectId}`;

/** The remembered tool id, or "" when there is none. Whether the tool still exists is the caller's to check. */
export function rememberedProfile(projectId: string | undefined) {
  if (!projectId) return "";
  try {
    return localStorage.getItem(key(projectId)) ?? "";
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
