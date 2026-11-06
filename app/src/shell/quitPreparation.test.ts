import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { ApiBackendProvider } from "../features/tools/api/backend";
import { dirtyEnvDraftCount, EnvEditor, resetEnvDrafts } from "../features/tools/api/EnvEditor";
import { emptyDraft } from "../features/tools/api/model/draft";
import { fileFromDraft } from "../features/tools/api/model/fileDraft";
import { confirmApiQuit, useQuitGuard } from "../features/tools/api/quitGuard";
import { isDirty, useApiTabs } from "../features/tools/api/store";
import { FakeBackend } from "../features/tools/api/test-backend";
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

const personal = { kind: "personal" } as const;
const documentsCheck = (safe: boolean) => ({
  prepareWindowClose: vi.fn(async () => safe),
  cancelWindowClose: vi.fn(),
  abortWindowClose: vi.fn(),
});
const asked = () => useQuitGuard.getState().request;
afterEach(() => {
  cleanup();
  asked()?.answer(false);
  resetEnvDrafts();
  useApiTabs.getState().reset();
});
/** An environment draft edited in the editor; like request tabs it lives only in this renderer. */
async function editEnvironment() {
  render(
    createElement(ApiBackendProvider, {
      backend: new FakeBackend(),
      children: createElement(EnvEditor, { root: personal }),
    }),
  );
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "새 환경" }));
  fireEvent.change(screen.getByLabelText("환경 이름"), { target: { value: "dev" } });
}

test("api_changes_cancelled_quit_acks_unsafe_without_asking_documents", async () => {
  useApiTabs.getState().openNew(personal);
  const documents = documentsCheck(true);
  const blocked = vi.fn();
  const ack = vi.fn(async (round: number, safe: boolean) => {
    // Native rolls a refused round back at once; its cancel may arrive before this ACK returns.
    if (!safe) handler.cancel(round);
  });
  const handler = createQuitPreparation(documents, ack, blocked, confirmApiQuit);
  const first = handler.prepare(1);
  expect(asked()).toMatchObject({ requests: 1, environments: 0 });
  asked()?.answer(false);
  await first;
  expect(ack.mock.calls).toEqual([[1, false]]);
  expect(documents.prepareWindowClose).not.toHaveBeenCalled();
  // [취소] is the user's answer, not a failure to save: no generic notice.
  expect(blocked).not.toHaveBeenCalled();
  const retry = handler.prepare(2);
  expect(asked()).toMatchObject({ requests: 1, environments: 0 });
  asked()?.answer(false);
  await retry;
  expect(ack.mock.calls).toEqual([
    [1, false],
    [2, false],
  ]);
  expect(documents.prepareWindowClose).not.toHaveBeenCalled();
  expect(blocked).not.toHaveBeenCalled();
});

test("api_changes_discarded_continue_to_documents", async () => {
  useApiTabs.getState().openNew(personal);
  const documents = documentsCheck(true);
  const ack = vi.fn(async () => {});
  const blocked = vi.fn();
  const handler = createQuitPreparation(documents, ack, blocked, confirmApiQuit);
  const pending = handler.prepare(1);
  expect(asked()).toBeDefined();
  expect(documents.prepareWindowClose).not.toHaveBeenCalled();
  asked()?.answer(true);
  await pending;
  expect(documents.prepareWindowClose).toHaveBeenCalledTimes(1);
  expect(ack.mock.calls).toEqual([[1, true]]);
  expect(blocked).not.toHaveBeenCalled();
});

test("no_api_changes_go_straight_to_documents", async () => {
  useApiTabs
    .getState()
    .openFile(personal, "saved.request.json", JSON.stringify(fileFromDraft(emptyDraft(), "A")), "d1");
  const documents = documentsCheck(true);
  const ack = vi.fn(async () => {});
  const handler = createQuitPreparation(documents, ack, vi.fn(), confirmApiQuit);
  const pending = handler.prepare(1);
  // Same turn as before this guard existed: nothing to ask, so documents start at once.
  expect(documents.prepareWindowClose).toHaveBeenCalledTimes(1);
  expect(asked()).toBeUndefined();
  await pending;
  expect(ack.mock.calls).toEqual([[1, true]]);
});

test("api_discard_keeps_drafts_when_documents_cancel", async () => {
  useApiTabs.getState().openNew(personal);
  await editEnvironment();
  const [tab] = useApiTabs.getState().tabs;
  expect(dirtyEnvDraftCount()).toBe(1);
  // The user discards API changes, then cancels at the documents prompt.
  const documents = documentsCheck(false);
  const ack = vi.fn(async () => {});
  const handler = createQuitPreparation(documents, ack, vi.fn(), confirmApiQuit);
  const pending = handler.prepare(1);
  expect(asked()).toMatchObject({ requests: 1, environments: 1 });
  act(() => asked()?.answer(true));
  await pending;
  expect(documents.prepareWindowClose).toHaveBeenCalledTimes(1);
  expect(ack.mock.calls).toEqual([[1, false]]);
  act(() => handler.cancel(1));
  expect(useApiTabs.getState().tabs).toEqual([tab]);
  expect(tab && isDirty(tab)).toBe(true);
  expect(dirtyEnvDraftCount()).toBe(1);
  expect(screen.getByLabelText("환경 이름")).toHaveValue("dev");
  // The changes are still there, so the next quit asks again.
  const retry = handler.prepare(2);
  expect(asked()).toMatchObject({ requests: 1, environments: 1 });
  act(() => asked()?.answer(false));
  await retry;
  expect(ack.mock.calls).toEqual([
    [1, false],
    [2, false],
  ]);
});

test("native_cancel_closes_an_open_api_prompt_without_an_ack", async () => {
  useApiTabs.getState().openNew(personal);
  const documents = documentsCheck(true);
  const ack = vi.fn(async () => {});
  const blocked = vi.fn();
  const handler = createQuitPreparation(documents, ack, blocked, confirmApiQuit);
  const pending = handler.prepare(1);
  expect(asked()).toBeDefined();
  // For example the 30 s native deadline, or another window failed this round.
  handler.cancel(1);
  expect(asked()).toBeUndefined();
  await pending;
  expect(ack).not.toHaveBeenCalled();
  expect(documents.prepareWindowClose).not.toHaveBeenCalled();
  expect(blocked).toHaveBeenCalledTimes(1);
  // The round is released, so the next one asks again.
  const retry = handler.prepare(2);
  expect(asked()).toBeDefined();
  asked()?.answer(true);
  await retry;
  expect(ack.mock.calls).toEqual([[2, true]]);
});
