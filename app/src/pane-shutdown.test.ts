import { ownedPaneSessions, waitPaneShutdown } from "../e2e/pane-shutdown";

const processes = vi.hoisted(() => new Map<number, string>());
const hooks = vi.hoisted(() => ({ afterList: undefined as (() => void) | undefined, missingCode: "ENOENT" }));
vi.mock("node:fs", () => {
  const fs = {
    readdirSync: () => {
      const ids = [...processes.keys()].map(String);
      hooks.afterList?.();
      return ids;
    },
    readFileSync: (path: string) => {
      const result = processes.get(Number(path.split("/")[2]));
      if (result === undefined) throw Object.assign(new Error("gone"), { code: hooks.missingCode });
      return result;
    },
  };
  return { ...fs, default: fs };
});
function stat(pid: number, session = pid, started = "1", state = "S") {
  return `${pid} (owned ) pane) ${[state, "1", String(pid), String(session), ...Array(15).fill("0"), started].join(" ")}`;
}
afterEach(() => {
  processes.clear();
  hooks.afterList = undefined;
  hooks.missingCode = "ENOENT";
});

test.each(["ENOENT", "ESRCH"])("%s cannot establish ownership but permits a fresh shutdown scan", async (code) => {
  hooks.missingCode = code;
  expect(() => ownedPaneSessions([100])).toThrow("Cannot establish owned pane");
  processes.set(100, stat(100));
  const owned = ownedPaneSessions([100]);
  let scans = 0;
  hooks.afterList = () => {
    scans++;
    processes.delete(100);
  };
  await expect(waitPaneShutdown(owned, 100)).resolves.toBeUndefined();
  expect(scans).toBe(3); // The raced snapshot cannot count as either stable empty scan.
});

test.each(["EACCES", "EIO"])("unexpected %s stat failures preserve the fixture", async (code) => {
  processes.set(100, stat(100));
  const owned = ownedPaneSessions([100]);
  hooks.missingCode = code;
  processes.delete(100);
  expect(() => ownedPaneSessions([100])).toThrow("gone");
  await expect(waitPaneShutdown(owned, 100)).rejects.toMatchObject({ code });
});

test("ownership requires the pane to lead its session and PID reuse never claims a new session", async () => {
  expect(() => ownedPaneSessions([100])).toThrow("Cannot establish owned pane");
  processes.set(100, stat(100, 99));
  expect(() => ownedPaneSessions([100])).toThrow("does not own");
  processes.set(100, stat(100));
  const owned = ownedPaneSessions([100]);
  processes.set(100, stat(100, 100, "2"));
  processes.set(101, stat(101, 100, "3"));
  await expect(waitPaneShutdown(owned, 100)).resolves.toBeUndefined();
});

test("surviving orphan finalizers fail bounded shutdown visibly; unrelated sessions and zombies do not block", async () => {
  processes.set(100, stat(100));
  const owned = ownedPaneSessions([100]);
  processes.delete(100);
  processes.set(101, stat(101, 100, "2"));
  await expect(waitPaneShutdown(owned, 0)).rejects.toThrow("preserving fixture HOME: 101");
  processes.set(101, stat(101, 100, "2", "Z"));
  processes.set(200, stat(200));
  await expect(waitPaneShutdown(owned, 100)).resolves.toBeUndefined();
});

test.each(["ENOENT", "ESRCH", "zombie"])(
  "a pane that forks after directory enumeration then becomes %s cannot hide its orphan",
  async (state) => {
    processes.set(100, stat(100));
    const owned = ownedPaneSessions([100]);
    hooks.afterList = () => {
      hooks.afterList = undefined;
      processes.set(101, stat(101, 100, "2"));
      if (state !== "zombie") {
        hooks.missingCode = state;
        processes.delete(100);
      } else processes.set(100, stat(100, 100, "1", "Z"));
    };
    await expect(waitPaneShutdown(owned, 100)).rejects.toThrow("preserving fixture HOME: 101");
  },
);

test("successive finalizer forks each require fresh enumeration", async () => {
  processes.set(100, stat(100));
  const owned = ownedPaneSessions([100]);
  let parent = 100;
  hooks.afterList = () => {
    processes.delete(parent);
    parent++;
    processes.set(parent, stat(parent, 100, String(parent)));
    if (parent === 102) hooks.afterList = undefined;
  };
  await expect(waitPaneShutdown(owned, 100)).rejects.toThrow("preserving fixture HOME: 102");
});
