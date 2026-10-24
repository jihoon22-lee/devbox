import { afterEach, beforeEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { NotesWire } from "../notes/test-wire";
import { commitSource, prepareRemote, retryUncertain, runGit, sourceTargetKey, useSourceGit } from "./gitState";

const target = { projectId: "p" };
const before = {
  branch: "main",
  headOid: "old",
  upstream: { remote: "origin", destinationRef: "refs/heads/main" },
  upstreamOid: "base",
};
const after = { ...before, headOid: "new" };
const expectState = { state: before, indexHash: "i", worktreeHash: "w" };
const clients: NotesWire[] = [];
afterEach(() => {
  for (const wire of clients.splice(0)) wire.client.stop();
});
function wire() {
  const wire = new NotesWire();
  clients.push(wire);
  wire.client.start();
  wire.handlers["git.commit"] = () => ({});
  wire.handlers["git.status"] = () => ({ expect: { ...expectState, state: after } });
  return wire;
}
beforeEach(() => useSourceGit.setState({ entries: {}, confirmation: undefined }));
test("commit then push captures a fresh post-commit remote expectation and does not push before confirmation", async () => {
  const w = wire();
  await Promise.resolve();
  await commitSource(w.client, { target, message: "change", amend: false, expect: expectState }, true);
  expect(w.requests("git.commit")).toHaveLength(1);
  expect(w.requests("git.push")).toHaveLength(0);
  expect(useSourceGit.getState().confirmation?.action).toEqual({
    method: "git.push",
    params: { target, expect: after, forceWithLease: false },
  });
});
test("post-commit refresh failure preserves committed outcome and retry only reads the snapshot", async () => {
  const w = wire();
  w.handlers["git.status"] = () => {
    throw new RpcError("git.source_failed");
  };
  await Promise.resolve();
  await commitSource(w.client, { target, message: "change", amend: false, expect: expectState }, true);
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.committed).toBe(true);
  w.handlers["git.status"] = () => ({ expect: { ...expectState, state: after } });
  await prepareRemote(w.client, target, "git.push", false);
  expect(w.requests("git.commit")).toHaveLength(1);
  expect(useSourceGit.getState().confirmation?.action.params.target).toEqual(target);
});
test("late mutation result stays with captured project and preserves completion-unknown operation", async () => {
  const w = wire();
  let finish!: (value: unknown) => void;
  w.handlers["git.fetch"] = () =>
    new Promise((resolve) => {
      finish = resolve;
    });
  await Promise.resolve();
  const job = runGit(w.client, { method: "git.fetch", params: { target } });
  await Promise.resolve();
  useSourceGit.setState({
    entries: {
      ...useSourceGit.getState().entries,
      [sourceTargetKey({ projectId: "q" })]: { busy: false, notice: "other" },
    },
  });
  finish({
    id: "job",
    target,
    state: "cancelled",
    error: null,
    current: null,
    remoteOid: null,
    completionUnknown: true,
  });
  await job;
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.operation?.completionUnknown).toBe(true);
  expect(useSourceGit.getState().entries[sourceTargetKey({ projectId: "q" })]?.notice).toBe("other");
});
test("an ambiguous commit retry keeps its request identity and blocks a different write", async () => {
  const w = wire();
  await Promise.resolve();
  let attempts = 0;
  w.handlers["git.commit"] = () => {
    if (++attempts === 1) w.close();
    return {};
  };
  const params = { target, message: "once", amend: false, expect: expectState };
  await commitSource(w.client, params, false);
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.uncertain?.requestId).toBeTruthy();
  w.handlers["git.fetch"] = () => ({
    id: "bad",
    target,
    state: "running",
    error: null,
    current: null,
    remoteOid: null,
    completionUnknown: false,
  });
  await runGit(w.client, { method: "git.fetch", params: { target } });
  expect(w.requests("git.fetch")).toHaveLength(0);
  w.connect();
  await Promise.resolve();
  await retryUncertain(w.client, target);
  const requests = w.sent.filter((item) => item.method === "git.commit");
  expect(requests).toHaveLength(2);
  expect((requests[0] as unknown as { clientRequestId: string }).clientRequestId).toBe(
    (requests[1] as unknown as { clientRequestId: string }).clientRequestId,
  );
  expect(useSourceGit.getState().entries[sourceTargetKey(target)]?.uncertain).toBeUndefined();
});
