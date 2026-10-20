import { execFileSync, spawn } from "node:child_process";
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

test.each([
  ["control attach", ["-C", "attach", "-t", "=owned"]],
  ["attach-session alias", ["-C", "attach-session", "-t", "=owned"]],
  ["compound grid attach and resize", ["-C", "attach", "-t", "=owned", ";", "refresh-client", "-C", "80,24"]],
])("diagnostic wrapper preserves the client PID for %s", async (_name, args) => {
  const home = mkdtempSync(join(tmpdir(), "devbox-tmux-client-"));
  const instance = `test-${randomBytes(6).toString("hex")}`;
  const socket = `devbox-${instance}`;
  const bin = join(home, "bin");
  mkdirSync(bin);
  const env: NodeJS.ProcessEnv = { ...process.env, HOME: home };
  delete env.TMUX;
  delete env.TMUX_TMPDIR;
  const diagnostics = new TmuxDiagnostics(home, instance, env);
  diagnostics.install(bin);
  env.PATH = `${bin}:${env.PATH}`;
  let client: ReturnType<typeof spawn> | undefined;
  try {
    execFileSync(
      "tmux",
      ["-L", socket, "-f", "/dev/null", "new-session", "-d", "-s", "owned", "-c", home, "sleep 30"],
      { env, timeout: 5000 },
    );
    client = spawn("tmux", ["-L", socket, "-f", "/dev/null", ...args], { env, stdio: ["pipe", "ignore", "ignore"] });
    const pid = client.pid;
    expect(pid).toBeGreaterThan(1);
    await expect
      .poll(
        () =>
          execFileSync(diagnostics.binary, ["-L", socket, "list-clients", "-F", "#{client_pid}"], {
            env,
            encoding: "utf8",
            timeout: 1000,
          })
            .trim()
            .split("\n"),
        { timeout: 2000 },
      )
      .toContain(String(pid));
  } finally {
    const exited =
      client && client.exitCode === null && client.signalCode === null
        ? new Promise<void>((resolve) => client!.once("exit", () => resolve()))
        : Promise.resolve();
    client?.kill();
    await stopOwnedTmux(instance, env);
    await exited;
    rmSync(home, { recursive: true, force: true });
  }
});
