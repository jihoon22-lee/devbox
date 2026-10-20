// @vitest-environment node
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RuntimeResources } from "../../../e2e/runtime-resources";

const fixture = vi.hoisted(() => ({
  commands: [] as { program: string; args: string[]; env: NodeJS.ProcessEnv }[],
  control: false,
  removed: [] as string[],
}));
vi.mock("node:child_process", () => ({
  execFileSync: vi.fn((program: string, args: string[], options: { env: NodeJS.ProcessEnv }) => {
    fixture.commands.push({ program, args, env: options.env });
    if (args.includes("set-property")) fixture.control = true;
    if (args.includes("list-units")) return "[]";
    if (args.includes("show")) return "MemoryHigh=6442450944\nMemoryMax=8589934592\nMemorySwapMax=1073741824\n";
    return "";
  }),
  spawnSync: vi.fn((program: string, args: string[], options: { env: NodeJS.ProcessEnv }) => {
    fixture.commands.push({ program, args, env: options.env });
    return { status: 0, stdout: "" };
  }),
}));
vi.mock("node:fs", async (original) => {
  const fs = await original<typeof import("node:fs")>();
  return {
    ...fs,
    readFileSync: (path: string, encoding: BufferEncoding) =>
      path === "/proc/self/status" ? "Cpus_allowed_list:\t4-11\n" : fs.readFileSync(path, encoding),
    lstatSync: (path: string) => {
      if (path.includes("systemd/user.control")) {
        if (!fixture.control) throw Object.assign(new Error("absent"), { code: "ENOENT" });
        return { isDirectory: () => true, isSymbolicLink: () => false };
      }
      return fs.lstatSync(path);
    },
    unlinkSync: (path: string) => {
      fixture.removed.push(path);
    },
    rmdirSync: (path: string) => {
      fixture.removed.push(path);
      fixture.control = false;
    },
  };
});
test("runtime-only fixture captures fixed env, bounds owned resources and removes exact owned drop-ins", () => {
  fixture.commands = [];
  fixture.removed = [];
  fixture.control = false;
  const home = mkdtempSync(join(tmpdir(), "devbox-runtime-fixture-unit-"));
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: join(home, "config"),
    DEVBOX_NO_SCOPE: "1",
    SECRET_FIXTURE: "not-captured",
  };
  mkdirSync(join(env.XDG_CONFIG_HOME, "devbox/test-abcd"), { recursive: true });
  try {
    const resources = new RuntimeResources("test-abcd", env);
    const captured = readFileSync(join(env.XDG_CONFIG_HOME, "devbox/test-abcd/env"), "utf8");
    expect(captured).toBe(`PATH="${home}/bin:/usr/local/bin:/usr/bin:/bin"\nLANG="C.UTF-8"\n`);
    expect(captured).not.toContain("not-captured");
    expect(env.DEVBOX_NO_SCOPE).toBeUndefined();
    resources.start();
    resources.close();
    const property = fixture.commands.find((command) => command.args.includes("set-property"));
    expect(property?.args).toContain("AllowedCPUs=4,5,6,7");
    expect(property?.args).toContain("MemorySwapMax=1G");
    expect(
      fixture.commands.some((command) => command.program === "docker" || command.args.includes("daemon-reload")),
    ).toBe(false);
    expect(fixture.removed).toHaveLength(6);
    expect(fixture.removed.every((path) => path.includes("devbox-test-abcd.slice.d"))).toBe(true);
    expect(
      fixture.commands
        .filter((command) => command.args.includes("stop"))
        .every((command) => command.args.at(-1)?.startsWith("devbox-test-abcd")),
    ).toBe(true);
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
});
