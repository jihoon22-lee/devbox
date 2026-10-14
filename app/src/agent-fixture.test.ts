import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { Stack } from "../e2e/support";

test("agent fixture owns Git identity, gamma repository, config and syntactically valid actual-hook script", () => {
  const fixture = new Stack();
  try {
    expect(fixture.env.HOME).toBe(fixture.home);
    expect(fixture.env.DEVBOX_NO_SCOPE).toBe("1");
    const repo = join(fixture.home, "projects/gamma");
    const git = (...args: string[]) =>
      execFileSync("git", ["-C", repo, ...args], { env: fixture.env })
        .toString()
        .trim();
    expect(git("branch", "--show-current")).toBe("main");
    expect(git("config", "user.name")).toBe("devbox-test");
    expect(git("config", "user.email")).toBe("test@devbox.invalid");
    expect(git("show", "HEAD:initial.txt")).toBe("initial");
    const config = join(fixture.env.XDG_CONFIG_HOME as string, "devbox");
    expect(readFileSync(join(config, "config.toml"), "utf8")).toContain('hooks="claude"');
    expect(existsSync(join(config, fixture.instance, "tmux.conf"))).toBe(true);
    expect(existsSync(join(fixture.home, ".config/devbox/config.toml"))).toBe(false);
    const script = join(fixture.home, "fake-tool.sh");
    execFileSync("bash", ["-n", script], { env: fixture.env });
    expect(readFileSync(script, "utf8")).toContain('"$DEVBOX_BIN" agent event --from claude');
    expect(fixture.children).toHaveLength(0);
  } finally {
    rmSync(fixture.home, { recursive: true, force: true });
  }
});
