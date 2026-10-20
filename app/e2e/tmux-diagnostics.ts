import { execFileSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readdirSync, readSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;

// Only installed into the fixture's private PATH, before its daemon starts.
export class TmuxDiagnostics {
  readonly directory: string;
  readonly observations: string[] = [];
  readonly binary: string;
  constructor(
    home: string,
    readonly instance: string,
    readonly env: NodeJS.ProcessEnv,
  ) {
    if (!/^test-[a-f0-9]{12}$/.test(instance)) throw new Error("Refusing an unowned tmux instance");
    this.directory = join(home, "tmux-diagnostics");
    this.binary = execFileSync("sh", ["-c", "command -v tmux"], { env, encoding: "utf8", timeout: 5000 }).trim();
    mkdirSync(this.directory);
    this.observations.push(execFileSync(this.binary, ["-V"], { env, encoding: "utf8", timeout: 5000 }).trim());
  }
  install(bin: string) {
    // -vv writes server/client/output logs in this owned directory. Preserve the caller's exit status.
    writeFileSync(
      join(bin, "tmux"),
      `#!/bin/sh\ncd ${quote(this.directory)} || exit 1\n${quote(this.binary)} -vv "$@"\nstatus=$?\nprintf 'exit=%s args=' "$status" >> commands.log\nprintf '%s ' "$@" >> commands.log\nprintf '\\n' >> commands.log\nexit "$status"\n`,
      { mode: 0o700 },
    );
  }
  capture(phase: string) {
    const env = { ...this.env };
    delete env.TMUX;
    delete env.TMUX_TMPDIR;
    try {
      const pid = execFileSync(this.binary, ["-L", `devbox-${this.instance}`, "display-message", "-p", "#{pid}"], {
        env,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        timeout: 5000,
      }).trim();
      this.observations.push(`${phase}: running pid=${pid}`);
    } catch (error) {
      const failure = error as Error & { status?: number; signal?: string; stderr?: Buffer };
      const stderr = failure.stderr?.toString() ?? failure.message;
      const status = /no server running|No such file or directory/.test(stderr) ? "absent" : "probe failed";
      this.observations.push(`${phase}: ${status}; probe exit=${failure.status}; signal=${failure.signal}; ${stderr}`);
    }
    try {
      const logs = readdirSync(this.directory)
        .filter((name) => name === "commands.log" || /^tmux-(server|client|out)-\d+\.log$/.test(name))
        .map((name) => {
          const path = join(this.directory, name);
          const size = statSync(path).size;
          const tail = Buffer.alloc(Math.min(size, 65536));
          const fd = openSync(path, "r");
          try {
            const length = readSync(fd, tail, 0, tail.length, Math.max(0, size - tail.length));
            return `${name} (${size} bytes; tail):\n${tail.subarray(0, length).toString()}`;
          } finally {
            closeSync(fd);
          }
        });
      return [...this.observations, ...logs].join("\n");
    } catch (error) {
      return [...this.observations, `Log collection failed: ${error}`].join("\n");
    }
  }
}
