import assert from "node:assert/strict";
import test from "node:test";
import { crashOwnedWorkspace } from "./windows-workspace-crash.mjs";
const owner = {
  Pid: 123,
  Created: "2026-10-05T00:00:00.0000000Z",
  Name: "devbox-workspace.exe",
  Path: "C:\\owned\\devbox-workspace.exe",
};
test("crash force-stops only identity-checked owner and waits native exit", async () => {
  const events = [];
  await crashOwnedWorkspace({
    owner,
    executable: owner.Path,
    child: { pid: 123 },
    run: (_exe, args) => {
      const command = args.at(-1);
      assert.match(command, /Stop-Process -Id 123 -Force/);
      assert.match(command, /CreationDate/);
      assert.match(command, /ExecutablePath/);
      assert.doesNotMatch(command, /CloseMainWindow|taskkill|\/T/);
      events.push("force");
      return { status: 0, stdout: "true" };
    },
    waitForExit: async () => events.push("exit"),
  });
  assert.deepEqual(events, ["force", "exit"]);
});
test("identity mismatch cannot force or await an unrelated child", async () => {
  let invoked = false;
  await assert.rejects(
    crashOwnedWorkspace({
      owner,
      executable: owner.Path,
      child: { pid: 124 },
      run: () => {
        invoked = true;
      },
      waitForExit: async () => {
        invoked = true;
      },
    }),
  );
  assert.equal(invoked, false);
  await assert.rejects(
    crashOwnedWorkspace({
      owner,
      executable: owner.Path,
      child: { pid: 123 },
      run: () => ({ status: 0, stdout: "false" }),
      waitForExit: async () => {
        invoked = true;
      },
    }),
  );
  assert.equal(invoked, false);
});
