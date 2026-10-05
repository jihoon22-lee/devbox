import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { observeNormalClose, requestNormalClose } from "./windows-suite-ui-context.mjs";
// Catalog runs without pnpm install; only the optional DOM cases need jsdom.
// Resolve separately so a broken installed package (or its dependency) still fails.
let jsdomPath;
try {
  jsdomPath = createRequire(import.meta.url).resolve("jsdom");
} catch (error) {
  if (error?.code !== "MODULE_NOT_FOUND" || !error.message.startsWith("Cannot find module 'jsdom'\n")) throw error;
}
const JSDOM = jsdomPath ? (await import(pathToFileURL(jsdomPath).href)).JSDOM : undefined;
if (jsdomPath) assert.equal(typeof JSDOM, "function", "Installed jsdom must export JSDOM");

test("native close requests once, confirms review, and waits for child exit before cleanup", async () => {
  const child = { exitCode: null };
  const events = [];
  await requestNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => events.includes("request") },
      ui: {
        click: async (target) => {
          assert.equal(target.name, "종료");
          events.push("confirm");
        },
      },
    },
    async () => events.push("request"),
    async (check) => {
      assert.equal(await check(), false);
      assert.deepEqual(events, ["request", "confirm"]);
      assert.equal(await check(), false);
      child.exitCode = 0;
      assert.equal(await check(), true);
      events.push("exit");
    },
  );
  events.push("cleanup");
  assert.deepEqual(events, ["request", "confirm", "exit", "cleanup"]);
});
test("reviewed close cannot continue to cleanup while the child remains alive", async () => {
  let cleanup = false;
  await assert.rejects(async () => {
    await requestNormalClose(
      {
        child: { exitCode: null },
        product: "workspace",
        cdp: { evaluate: async () => true },
        ui: { click: async () => {} },
      },
      async () => {},
      async (check) => {
        assert.equal(await check(), false);
        throw new Error("owned close deadline");
      },
    );
    cleanup = true;
  }, /owned close deadline/);
  assert.equal(cleanup, false);
});
test("owned native close waits child exit after renderer disconnect without another renderer call", async () => {
  const child = { exitCode: null };
  let calls = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: {
        evaluate: async () => {
          calls++;
          throw new Error("CDP disconnected");
        },
      },
      ui: {},
    },
    async (check) => {
      assert.equal(await check(), false);
      child.exitCode = 0;
      assert.equal(await check(), true);
    },
  );
  assert.equal(calls, 1);
});
test("renderer disconnect is not proof that the owned process exited", async () => {
  const child = { exitCode: null };
  await assert.rejects(
    observeNormalClose(
      {
        child,
        product: "workspace",
        cdp: {
          evaluate: async () => {
            throw new Error("CDP disconnected");
          },
        },
        ui: {},
      },
      async (check) => {
        assert.equal(await check(), false);
        assert.equal(await check(), false);
        throw new Error("owned close deadline");
      },
    ),
    /owned close deadline/,
  );
});
test("Workspace close still requires the explicit owned quit review", async () => {
  const child = { exitCode: null };
  let accepted = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => true },
      ui: {
        click: async (target) => {
          assert.deepEqual(target, {
            role: "button",
            name: "종료",
            scope: { role: "dialog", name: "Workspace 종료 검토" },
          });
          accepted++;
          child.exitCode = 0;
        },
      },
    },
    async (check) => {
      assert.equal(await check(), true);
    },
  );
  assert.equal(accepted, 1);
});

test("close diagnostics distinguish submitted review from missing review without claiming exit", async () => {
  for (const present of [false, true]) {
    const observations = [];
    await assert.rejects(
      observeNormalClose(
        {
          child: { exitCode: null },
          product: "workspace",
          cdp: { evaluate: async () => present },
          ui: { click: async () => {} },
          reportClose: (value) => observations.push(value),
        },
        async (check) => {
          assert.equal(await check(), false);
          throw new Error("owned close deadline");
        },
      ),
      /owned close deadline/,
    );
    assert.deepEqual(observations.at(-1), {
      reviewObserved: present,
      reviewSubmitted: present,
      rendererDisconnected: false,
      childExited: false,
    });
  }
});

test("close diagnostics preserve CDP timeout and never classify it as renderer shutdown", async () => {
  let last;
  const failure = new Error("CDP setup timeout");
  await assert.rejects(
    observeNormalClose(
      {
        child: { exitCode: null },
        product: "workspace",
        cdp: {
          evaluate: async () => {
            throw failure;
          },
        },
        ui: {},
        reportClose: (value) => {
          last = value;
        },
      },
      async (check) => check(),
    ),
    (error) => error === failure,
  );
  assert.equal(last.rendererDisconnected, false);
  assert.equal(last.childExited, false);
});

test("explicit owned handoff close review still waits for native process exit", async () => {
  const child = { exitCode: null };
  let reviews = 0;
  await observeNormalClose(
    {
      child,
      product: "workspace",
      cdp: { evaluate: async () => true },
      ui: {
        click: async () => {
          throw new Error("Unscoped clean-close button used");
        },
      },
      reviewWorkspaceClose: async () => {
        reviews++;
      },
    },
    async (check) => {
      assert.equal(await check(), false);
      assert.equal(reviews, 1);
      assert.equal(await check(), false);
      assert.equal(reviews, 1);
      child.exitCode = 0;
      assert.equal(await check(), true);
    },
  );
});

for (const [name, markup, expected] of [
  ["native open review", '<dialog open aria-label="Workspace 종료 검토"><button>종료</button></dialog>', true],
  [
    "retained legacy review",
    '<section role="dialog" aria-label="Workspace 종료 검토"><button>종료</button></section>',
    true,
  ],
  ["closed native review", '<dialog aria-label="Workspace 종료 검토"><button>종료</button></dialog>', false],
  [
    "closed explicitly named native review",
    '<dialog role="dialog" aria-label="Workspace 종료 검토"><button>종료</button></dialog>',
    false,
  ],
  ["other open dialog", '<dialog open aria-label="다른 검토"><button>종료</button></dialog>', false],
]) {
  test(`normal close recognizes only actionable Workspace review: ${name}`, {
    skip: JSDOM ? false : "jsdom is not installed; DOM cases run after pnpm install",
  }, async () => {
    const dom = new JSDOM(markup);
    const child = { exitCode: null };
    let clicks = 0;
    try {
      await observeNormalClose(
        {
          child,
          product: "workspace",
          cdp: { evaluate: async (script) => new Function("document", `return ${script}`)(dom.window.document) },
          ui: {
            click: async () => {
              clicks++;
            },
          },
        },
        async (check) => {
          assert.equal(await check(), false, "review submission never claims process exit");
          assert.equal(clicks, expected ? 1 : 0);
          assert.equal(await check(), false);
          assert.equal(clicks, expected ? 1 : 0, "never resubmit the close action");
          child.exitCode = 0;
          assert.equal(await check(), true);
        },
      );
    } finally {
      dom.window.close();
    }
  });
}
