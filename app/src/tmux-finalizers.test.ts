import { execFile } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { promisify } from "node:util";
import { stopOwnedTmux } from "../e2e/tmux-cleanup";

const run = promisify(execFile);
test.each([false, true])(
  "owned tmux teardown waits for delayed HUP finalizers before deleting HOME (orphan=%s)",
  async (orphan) => {
    const home = mkdtempSync(join(tmpdir(), "devbox-tmux-finalizer-"));
    const instance = `test-${randomBytes(6).toString("hex")}`;
    const env: NodeJS.ProcessEnv = { ...process.env, HOME: home };
    delete env.TMUX;
    delete env.TMUX_TMPDIR;
    const ready = join(home, "ready");
    const done = join(home, "finalized");
    const script = join(home, "pane.sh");
    writeFileSync(
      join(home, "finalizer.sh"),
      'printf started > "$HOME/finalizer-started"\nsleep 0.25\nprintf done > "$HOME/finalized"\n',
    );
    const finalizer = orphan
      ? 'nohup bash "$HOME/finalizer.sh" >/dev/null 2>&1 & disown; until test -f "$HOME/finalizer-started"; do :; done'
      : 'bash "$HOME/finalizer.sh"';
    writeFileSync(
      script,
      `trap '${finalizer}; exit' HUP\nprintf ready > "$HOME/ready"\nwhile true; do read -r line || :; done\n`,
    );
    const waitFor = async (path: string) => {
      const deadline = Date.now() + 3000;
      while (!existsSync(path) && Date.now() < deadline) await delay(10);
      expect(existsSync(path)).toBe(true);
    };
    try {
      await run(
        "tmux",
        ["-L", `devbox-${instance}`, "-f", "/dev/null", "new-session", "-d", "bash", "--noprofile", "--norc", script],
        { env, timeout: 5000 },
      );
      await waitFor(ready);
      await stopOwnedTmux(instance, env);
      expect(existsSync(done)).toBe(true);
    } finally {
      await stopOwnedTmux(instance, env);
      await waitFor(done);
      rmSync(home, { recursive: true, force: true });
    }
  },
  10000,
);
