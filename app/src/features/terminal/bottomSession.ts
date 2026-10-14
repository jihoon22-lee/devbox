const inflight = new WeakMap<object, Map<string, Promise<string>>>();
const remembered = new WeakMap<object, Map<string, string>>();
export function readBottomSession(client: object, project: string): string | undefined {
  const memory = remembered.get(client)?.get(project);
  if (memory) return memory;
  try {
    return localStorage.getItem(`devbox.bottomTerminal.${project}`) ?? undefined;
  } catch {
    return undefined;
  }
}
export function rememberBottomSession(client: object, project: string, id?: string) {
  let projects = remembered.get(client);
  if (!projects) {
    projects = new Map();
    remembered.set(client, projects);
  }
  if (id) projects.set(project, id);
  else projects.delete(project);
  try {
    if (id) localStorage.setItem(`devbox.bottomTerminal.${project}`, id);
    else localStorage.removeItem(`devbox.bottomTerminal.${project}`);
  } catch {
    /* Optional storage; keep the ID in memory. */
  }
}
export function ensureBottomSession(client: object, project: string, create: () => Promise<string>): Promise<string> {
  let projects = inflight.get(client);
  if (!projects) {
    projects = new Map();
    inflight.set(client, projects);
  }
  const existing = projects.get(project);
  if (existing) return existing;
  const pending = create().then((id) => {
    rememberBottomSession(client, project, id);
    return id;
  });
  projects.set(project, pending);
  // Retain only running operations; persistent IDs live in optional storage/query cache.
  void pending
    .finally(() => {
      if (projects.get(project) === pending) projects.delete(project);
    })
    .catch(() => {});
  return pending;
}
