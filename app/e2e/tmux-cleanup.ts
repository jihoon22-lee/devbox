import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);
export async function stopOwnedTmux(instance: string, env: NodeJS.ProcessEnv) {
  if (!/^test-[a-f0-9]{12}$/.test(instance)) throw new Error("Refusing an unowned tmux instance");
  const cleanEnv = { ...env };
  delete cleanEnv.TMUX;
  delete cleanEnv.TMUX_TMPDIR;
  try {
    await run("tmux", ["-L", `devbox-${instance}`, "kill-server"], { env: cleanEnv, timeout: 5_000, maxBuffer: 4096 });
  } catch (error) {
    const failure = error as NodeJS.ErrnoException & { stderr?: string };
    // No terminal was created, or the owned server already exited.
    if (failure.code === "ENOENT" || /no server running|No such file or directory/.test(failure.stderr ?? "")) return;
    throw error;
  }
}
