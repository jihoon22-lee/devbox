import test from "node:test";
import assert from "node:assert/strict";
import {
  dependencyNetworkScope,
  withDependencyNetworkBlock,
  dependencyProviderFailureVisible,
} from "./windows-dependency-network.mjs";
const root = `C:\\runner\\temp\\devbox-suite-delivery-${"a".repeat(32)}\\Suite UI Fixture`;
const owner = {
  fixtureRoot: root,
  identity: { Pid: 123, Path: `${root}\\generations\\fixture\\products\\workspace\\devbox-workspace.exe` },
  started: "2026-10-05T00:00:00.0000000Z",
};
const hosted = {
  GITHUB_ACTIONS: "true",
  RUNNER_ENVIRONMENT: "github-hosted",
  RUNNER_OS: "Windows",
  GITHUB_REPOSITORY: "jihoon22-lee/devbox",
  GITHUB_RUN_ID: "123",
  RUNNER_TEMP: "C:\\runner\\temp",
};
test("scope rejects local/self-hosted and foreign image before a firewall command", () => {
  for (const env of [{}, { ...hosted, RUNNER_ENVIRONMENT: "self-hosted" }])
    assert.throws(() => dependencyNetworkScope(owner, "b".repeat(64), env));
  assert.throws(() =>
    dependencyNetworkScope(
      { ...owner, identity: { ...owner.identity, Path: "C:\\Windows\\app.exe" } },
      "b".repeat(64),
      hosted,
    ),
  );
  const first = dependencyNetworkScope(owner, "b".repeat(64), hosted);
  const second = dependencyNetworkScope(owner, "b".repeat(64), hosted);
  assert.notEqual(first.ruleName, second.ruleName);
});
test("one verified rule brackets the action and removes only its exact scope on failure", async () => {
  const scope = { ruleName: "owned" },
    events = [],
    failure = new Error("native provider observation failed");
  await assert.rejects(
    withDependencyNetworkBlock(
      scope,
      async () => {
        events.push("action");
        throw failure;
      },
      async (action, value) => {
        assert.equal(value, scope);
        events.push(action);
      },
    ),
    (error) => error === failure,
  );
  assert.deepEqual(events, ["Add", "action", "Remove"]);
});
test("failed creation never approves transmission or removes preexisting rules", async () => {
  const events = [];
  await assert.rejects(
    withDependencyNetworkBlock(
      {},
      async () => events.push("action"),
      async (action) => {
        events.push(action);
        throw new Error("creation denied");
      },
    ),
    /creation denied/,
  );
  assert.deepEqual(events, ["Add"]);
});
test("cleanup failure prevents PASS and retains the original action failure", async () => {
  const first = new Error("first"),
    cleanup = new Error("cleanup");
  await assert.rejects(
    withDependencyNetworkBlock(
      {},
      async () => {
        throw first;
      },
      async (action) => {
        if (action === "Remove") throw cleanup;
      },
    ),
    (error) => error instanceof AggregateError && error.errors[0] === first && error.errors[1] === cleanup,
  );
  await assert.rejects(
    withDependencyNetworkBlock(
      {},
      async () => "pass",
      async (action) => {
        if (action === "Remove") throw cleanup;
      },
    ),
    (error) => error === cleanup,
  );
});

test("provider failure requires actual positive transmission and failure counts in the same result", () => {
  const observed = (...text) =>
    dependencyProviderFailureVisible({ querySelectorAll: () => text.map((textContent) => ({ textContent })) });
  assert.equal(observed("전송 0 · 실패 0"), false);
  assert.equal(observed("전송 1 · 실패 0"), false);
  assert.equal(observed("전송 0 · 실패 1"), false);
  assert.equal(observed("전송 1 · 실패 0", "전송 0 · 실패 1"), false);
  assert.equal(observed("OSV 대상 1 · 전송 1 · 캐시 0 · stale 0 · 실패 1 · 생략 0"), true);
});
