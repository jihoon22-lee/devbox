import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { ownedPaneSessions, waitPaneShutdown } from "./pane-shutdown";

const run = promisify(execFile);
function absent(error: unknown) {
  const failure = error as NodeJS.ErrnoException & { stderr?: string };
  return failure.code === "ENOENT" || /no server running|No such file or directory/.test(failure.stderr ?? "");
}
export async function stopOwnedTmux(instance: string, env: NodeJS.ProcessEnv) {
  if (!/^test-[a-f0-9]{12}$/.test(instance)) throw new Error("Refusing an unowned tmux instance");
  const cleanEnv = { ...env };
  delete cleanEnv.TMUX;
  delete cleanEnv.TMUX_TMPDIR;
  let panes: ReturnType<typeof ownedPaneSessions> = [];
  try {
    const { stdout } = await run("tmux", ["-L", `devbox-${instance}`, "list-panes", "-a", "-F", "#{pane_pid}"], {
      env: cleanEnv,
      timeout: 5_000,
      maxBuffer: 4096,
    });
    const pids = stdout.trim() ? stdout.trim().split(/\s+/).map(Number) : [];
    if (pids.some((pid) => !Number.isSafeInteger(pid) || pid <= 0)) throw new Error("Invalid owned tmux pane PID");
    panes = ownedPaneSessions(pids);
  } catch (error) {
    if (absent(error)) return;
    // Closing the last task leaves an owned server with no current pane.
    // Still kill that server; do not turn other listing errors into success.
    if ((error as { stderr?: string }).stderr?.trim() !== "no current target") throw error;
  }
  try {
    await run("tmux", ["-L", `devbox-${instance}`, "kill-server"], { env: cleanEnv, timeout: 5_000, maxBuffer: 4096 });
  } catch (error) {
    if (!absent(error)) throw error;
  }
  await waitPaneShutdown(panes);
}
