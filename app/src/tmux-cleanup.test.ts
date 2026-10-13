import { promisify } from "node:util";
import { afterEach, expect, test, vi } from "vitest";
import { stopOwnedTmux } from "../e2e/tmux-cleanup";

const mock = vi.hoisted(() => ({ run: vi.fn(), error: null as null | Error }));
vi.mock("node:child_process", () => {
  const execFile = (
    command: string,
    args: string[],
    options: unknown,
    callback: (error: Error | null, stdout: string, stderr: string) => void,
  ) => {
    mock.run(command, args, options);
    callback(mock.error, "", "");
  };
  Object.defineProperty(execFile, promisify.custom, {
    value: async (...args: unknown[]) => {
      mock.run(...args);
      if (mock.error) throw mock.error;
      return { stdout: "", stderr: "" };
    },
  });
  return { execFile, default: { execFile } };
});
afterEach(() => {
  mock.run.mockClear();
  mock.error = null;
});
test("cleanup targets only the exact owned tmux socket with a timeout and clean environment", async () => {
  await stopOwnedTmux("test-0123456789ab", { HOME: "/owned", TMUX: "shared", TMUX_TMPDIR: "/shared", PATH: "/bin" });
  expect(mock.run).toHaveBeenCalledWith("tmux", ["-L", "devbox-test-0123456789ab", "kill-server"], {
    env: { HOME: "/owned", PATH: "/bin" },
    timeout: 5000,
    maxBuffer: 4096,
  });
  await expect(stopOwnedTmux("main", {})).rejects.toThrow("unowned");
  expect(mock.run).toHaveBeenCalledTimes(2);
});
test("absent owned servers are harmless; unexpected cleanup failures remain failures", async () => {
  mock.error = Object.assign(new Error("absent"), { stderr: "no server running on /tmp/owned" });
  await expect(stopOwnedTmux("test-0123456789ab", {})).resolves.toBeUndefined();
  mock.error = Object.assign(new Error("permission denied"), { stderr: "permission denied" });
  await expect(stopOwnedTmux("test-0123456789ab", {})).rejects.toThrow("permission denied");
});
