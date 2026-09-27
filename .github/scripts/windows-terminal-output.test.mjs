import assert from "node:assert/strict";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { terminalOutputExpression, terminalPromptVisible } from "./windows-terminal-output.mjs";

test("output fixture subscribes with a channel and releases it after a batch or idle deadline", async () => {
  for (const idle of [false, true]) {
    let callback,
      unregistered = 0;
    const calls = [];
    const expected = {
      frames: [{ sequence: 4, data: "fixture" }],
      cursor: 4,
      truncated: false,
      more: false,
      closed: false,
    };
    const result = await runInNewContext(terminalOutputExpression("owned", 3, 1), {
      window: {
        __TAURI_INTERNALS__: {
          transformCallback: (fn) => {
            callback = fn;
            return 42;
          },
          unregisterCallback: (id) => {
            assert.equal(id, 42);
            unregistered++;
          },
          invoke: async (command, input) => {
            calls.push([command, input]);
            if (command.endsWith("terminal_describe"))
              return { handshake: { installationId: "i", sessionId: "s" }, context: null };
            if (command.endsWith("terminal_output_stream")) {
              assert.equal(input.channel, "__CHANNEL__:42");
              assert.equal(input.request.method, "subscribe");
              assert.deepEqual(JSON.parse(JSON.stringify(input.request.args)), { sessionId: "owned", after: 3 });
              if (!idle) callback({ index: 0, message: expected });
              return { subscriptionId: "sub" };
            }
            assert.equal(input.request.method, "unsubscribe_terminal_output");
            assert.equal(input.request.args.subscriptionId, "sub");
          },
        },
      },
      crypto: { randomUUID: () => "r" },
      Date,
      setTimeout,
      clearTimeout,
    });
    assert.deepEqual(
      JSON.parse(JSON.stringify(result)),
      idle ? { frames: [], cursor: 3, truncated: false, more: false, closed: false } : expected,
    );
    assert.equal(unregistered, 1);
    assert.equal(calls.length, 3);
  }
});
test("output fixture removes its callback if subscription admission rejects", async () => {
  let unregistered = false;
  await assert.rejects(
    runInNewContext(terminalOutputExpression("foreign", 0, 1), {
      window: {
        __TAURI_INTERNALS__: {
          transformCallback: () => 42,
          unregisterCallback: () => {
            unregistered = true;
          },
          invoke: async (command) => {
            if (command.endsWith("terminal_describe"))
              return { handshake: { installationId: "i", sessionId: "s" }, context: null };
            throw new Error("denied");
          },
        },
      },
      crypto: { randomUUID: () => "r" },
      Date,
      setTimeout,
      clearTimeout,
    }),
    /denied/,
  );
  assert.equal(unregistered, true);
});

test("native ConPTY prompt accepts cursor movement replacing its trailing blank", () => {
  assert.equal(terminalPromptVisible("root@fixture:/owned#\x1b[K\x1b[1C\x1b]0;root@fixture: /owned\x07"), true);
  assert.equal(terminalPromptVisible("user@fixture:~$ "), true);
  assert.equal(terminalPromptVisible("\x1b[?25hstarting shell"), false);
});

test("output subscription retries only bounded pre-admission busy failures", async () => {
  for (const problem of ["transient", "busy", "denied"]) {
    let attempts = 0,
      callback,
      removed = 0,
      nextId = 0;
    const headers = [];
    const result = runInNewContext(terminalOutputExpression("owned", 0, 1), {
      window: {
        __TAURI_INTERNALS__: {
          transformCallback: (fn) => {
            callback = fn;
            return 42;
          },
          unregisterCallback: () => {
            removed++;
          },
          invoke: async (command, input) => {
            if (command.endsWith("terminal_describe"))
              return { handshake: { installationId: "i", sessionId: "s" }, context: null };
            if (command.endsWith("terminal_execute")) return true;
            headers.push(input.request.header);
            attempts++;
            if (problem !== "transient" || attempts < 3) throw problem === "transient" ? "busy" : problem;
            callback({ index: 0, message: { cursor: 1, frames: [] } });
            return { subscriptionId: "sub" };
          },
        },
      },
      crypto: { randomUUID: () => String(++nextId) },
      Date: { now: () => 1000 },
      setTimeout: (fn) => {
        fn();
        return 1;
      },
      clearTimeout: () => {},
    });
    if (problem === "transient") assert.equal((await result).cursor, 1);
    else await assert.rejects(result, (error) => error === problem);
    assert.equal(attempts, problem === "transient" ? 3 : problem === "busy" ? 20 : 1);
    assert.equal(removed, 1);
    assert.equal(new Set(headers.map((header) => header.requestId)).size, attempts);
    assert.ok(headers.every((header) => header.deadlineMs === 30000));
  }
});
