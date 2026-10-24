import { expect, test, vi } from "vitest";
import type { Methods } from "../rpc/gen/rpc";
import { createQuitPreparation } from "./quitPreparation";

test("waits for durable preparation before acknowledging and rolls back a cancelled in-flight round", async () => {
  let finish!: (safe: boolean) => void;
  const prepareWindowClose = vi.fn(
    () =>
      new Promise<boolean>((resolve) => {
        finish = resolve;
      }),
  );
  const cancelWindowClose = vi.fn();
  const ack = vi.fn(async () => {});
  const blocked = vi.fn();
  const handler = createQuitPreparation({ prepareWindowClose, cancelWindowClose }, ack, blocked);
  const pending = handler.prepare(1);
  expect(ack).not.toHaveBeenCalled();
  handler.cancel(1);
  finish(true);
  await pending;
  expect(ack).not.toHaveBeenCalled();
  expect(cancelWindowClose).toHaveBeenCalled();
});

test("a cancelled slow save stays frozen until it settles; duplicate and stale rounds cannot unfreeze it", async () => {
  let finish!: (safe: boolean) => void;
  const prepareWindowClose = vi.fn(
    () =>
      new Promise<boolean>((resolve) => {
        finish = resolve;
      }),
  );
  const cancelWindowClose = vi.fn();
  const ack = vi.fn(async () => {});
  const handler = createQuitPreparation({ prepareWindowClose, cancelWindowClose }, ack, vi.fn());
  const pending = handler.prepare(1);
  await handler.prepare(1);
  await handler.prepare(2);
  expect(prepareWindowClose).toHaveBeenCalledTimes(1);
  expect(ack).toHaveBeenCalledWith(2, false);
  handler.cancel(2);
  expect(cancelWindowClose).not.toHaveBeenCalled();
  handler.cancel(1);
  expect(cancelWindowClose).not.toHaveBeenCalled();
  finish(true);
  await pending;
  expect(cancelWindowClose).toHaveBeenCalledTimes(1);
});

test("ambiguous ACK failure stays frozen until native rollback", async () => {
  const cancelWindowClose = vi.fn();
  const handler = createQuitPreparation(
    { prepareWindowClose: async () => true, cancelWindowClose },
    async () => {
      throw new Error("response lost");
    },
    vi.fn(),
  );
  await handler.prepare(1);
  expect(cancelWindowClose).not.toHaveBeenCalled();
  handler.cancel(1);
  expect(cancelWindowClose).toHaveBeenCalledTimes(1);
});

import { keyId, memoryJournal } from "../features/documents/offlineJournal";
import { FakeDocumentsDaemon } from "../features/documents/test-daemon";

async function documentWindow(daemon: FakeDocumentsDaemon, name: string) {
  vi.resetModules();
  const { useDocuments } = await import("../features/documents/store");
  const key = { root: "vault", path: `${name}.md` };
  daemon.write(key, `${name} old`);
  const journal = memoryJournal();
  const stop = useDocuments.getState().connect(daemon, { journal, windowId: name });
  useDocuments.getState().open(key, { kind: "note" });
  await useDocuments.getState().settled();
  return { store: useDocuments, key, journal, stop };
}

test("main and popout edits at 100 ms reach disk before transport teardown, including an older deferred save", async () => {
  const daemon = new FakeDocumentsDaemon();
  const main = await documentWindow(daemon, "main");
  const popout = await documentWindow(daemon, "popout-note-a");
  vi.useFakeTimers();
  try {
    const original = daemon.call.bind(daemon);
    let release!: () => void;
    let first = true;
    vi.spyOn(daemon, "call").mockImplementation((method, params) => {
      if (method === "documents.save" && first) {
        first = false;
        return new Promise<void>((resolve) => {
          release = resolve;
        }).then(() => original(method, params as Methods["documents.save"]["params"]));
      }
      return original(method, params);
    });
    main.store.getState().edit(main.key, "older main");
    main.store.getState().save(main.key);
    main.store.getState().edit(main.key, "latest main");
    popout.store.getState().edit(popout.key, "latest popout");
    await vi.advanceTimersByTimeAsync(100);
    expect(await main.journal.list()).toEqual([]);
    expect(await popout.journal.list()).toEqual([]);
    const acks = new Set<string>();
    let exited = false;
    const handler = (name: string, doc: typeof main) =>
      createQuitPreparation(
        doc.store.getState(),
        async (_round, safe) => {
          expect(safe).toBe(true);
          acks.add(name);
          if (acks.size === 2) {
            expect(daemon.files.get(keyId(main.key))?.text).toBe("latest main");
            expect(daemon.files.get(keyId(popout.key))?.text).toBe("latest popout");
            exited = true;
            daemon.disconnect();
            main.stop();
            popout.stop();
          }
        },
        vi.fn(),
      );
    const prepareMain = handler("main", main).prepare(1);
    const preparePopout = handler("popout", popout).prepare(1);
    await popout.store.getState().settled();
    expect(exited).toBe(false);
    expect(main.store.getState().windowClosing).toBe(true);
    main.store.getState().edit(main.key, "edit during exit");
    release();
    await Promise.all([prepareMain, preparePopout]);
    expect(exited).toBe(true);
    expect(daemon.files.get(keyId(main.key))?.text).toBe("latest main");
    expect(daemon.files.get(keyId(popout.key))?.text).toBe("latest popout");
  } finally {
    main.stop();
    popout.stop();
    vi.useRealTimers();
  }
});

test("failed persistence preserves the dirty buffer and rolls back already prepared windows for retry", async () => {
  const daemon = new FakeDocumentsDaemon();
  const main = await documentWindow(daemon, "main");
  const popout = await documentWindow(daemon, "popout-note-a");
  vi.useFakeTimers();
  try {
    main.store.getState().edit(main.key, "latest main");
    popout.store.getState().edit(popout.key, "latest popout");
    const mainAck = vi.fn(async () => {});
    const popoutAck = vi.fn(async () => {});
    const a = createQuitPreparation(main.store.getState(), mainAck, vi.fn());
    const b = createQuitPreparation(popout.store.getState(), popoutAck, vi.fn());
    await a.prepare(1);
    daemon.failing.set("documents.save", "internal");
    await b.prepare(1);
    expect(mainAck).toHaveBeenCalledWith(1, true);
    expect(popoutAck).toHaveBeenCalledWith(1, false);
    expect(popout.store.getState().find(popout.key)?.text).toBe("latest popout");
    a.cancel(1);
    b.cancel(1);
    await main.store.getState().settled();
    expect(main.store.getState().windowClosing).toBe(false);
    expect(main.store.getState().find(main.key)?.text).toBe("latest main");
    expect(popout.store.getState().windowClosing).toBe(false);
    daemon.failing.clear();
    main.store.getState().edit(main.key, "main edited after rollback");
    popout.store.getState().edit(popout.key, "popout edited after rollback");
    expect(main.store.getState().find(main.key)?.state).toBe("Dirty");
    expect(popout.store.getState().find(popout.key)?.text).toBe("popout edited after rollback");
    await Promise.all([a.prepare(2), b.prepare(2)]);
    expect(mainAck).toHaveBeenCalledWith(2, true);
    expect(popoutAck).toHaveBeenCalledWith(2, true);
    expect(daemon.files.get(keyId(popout.key))?.text).toBe("popout edited after rollback");
    expect(daemon.files.get(keyId(main.key))?.text).toBe("main edited after rollback");
  } finally {
    main.stop();
    popout.stop();
    vi.useRealTimers();
  }
});

test("startup without a connected document session safely acknowledges an empty main window", async () => {
  vi.resetModules();
  const { useDocuments } = await import("../features/documents/store");
  const ack = vi.fn(async () => {});
  const handler = createQuitPreparation(useDocuments.getState(), ack, vi.fn());
  await handler.prepare(1);
  expect(ack).toHaveBeenCalledWith(1, true);
  handler.cancel(1);
  expect(useDocuments.getState().windowClosing).toBe(false);
});
