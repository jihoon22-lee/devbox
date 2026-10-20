import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stopOwnedTmux } from "../e2e/tmux-cleanup";
import { TmuxDiagnostics } from "../e2e/tmux-diagnostics";

test("owned tmux diagnostics retain version, verbose server logs and teardown status", async () => {
  const home = mkdtempSync(join(tmpdir(), "devbox-tmux-diagnostics-"));
  const instance = `test-${randomBytes(6).toString("hex")}`;
  const bin = join(home, "bin");
  mkdirSync(bin);
  const env: NodeJS.ProcessEnv = { ...process.env, HOME: home };
  delete env.TMUX;
  delete env.TMUX_TMPDIR;
  const diagnostics = new TmuxDiagnostics(home, instance, env);
  try {
    diagnostics.install(bin);
    env.PATH = `${bin}:${env.PATH}`;
    execFileSync("tmux", ["-L", `devbox-${instance}`, "-f", "/dev/null", "new-session", "-d", "-c", home, "sleep 30"], {
      env,
      timeout: 5000,
    });
    await expect
      .poll(() =>
        execFileSync("tmux", ["-L", `devbox-${instance}`, "display-message", "-p", "#{pane_current_path}"], {
          env,
          encoding: "utf8",
        }).trim(),
      )
      .toBe(home);
    diagnostics.capture("before-cleanup");
    await stopOwnedTmux(instance, env);
    const report = diagnostics.capture("after-cleanup");
    expect(report).toMatch(/tmux \d/);
    expect(report).toContain("before-cleanup: running");
    expect(report).toContain("after-cleanup: absent");
    expect(report).toMatch(/tmux-server-\d+\.log/);
    expect(report).toContain("exit=0");
    expect(report).toContain("kill-server");
    env.PATH = process.env.PATH;
    rmSync(diagnostics.directory, { recursive: true, force: true });
    expect(diagnostics.capture("missing-logs")).toContain("Log collection failed:");
  } finally {
    await stopOwnedTmux(instance, env);
    rmSync(home, { recursive: true, force: true });
  }
});

test("diagnostics refuse unowned tmux names before creating a wrapper", () => {
  expect(() => new TmuxDiagnostics("/unused", "prod", {})).toThrow("unowned");
});
