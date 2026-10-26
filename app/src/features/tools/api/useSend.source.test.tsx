import { act, cleanup, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./backend";
import { emptyDraft } from "./model/draft";
import { useApiTabs } from "./store";
import { FakeBackend } from "./test-backend";
import { useSend } from "./useSend";

afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
});
it("an unsaved project draft sends its captured root without a fabricated collection path", async () => {
  const root = { kind: "project", projectId: "owned-a" } as const;
  useApiTabs.getState().openNew(root);
  const tab = useApiTabs.getState().tabs[0]!;
  useApiTabs.getState().setDraftById(tab.id, { ...emptyDraft(), url: "http://owned.invalid/response" });
  const backend = new FakeBackend();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <ApiBackendProvider backend={backend}>{children}</ApiBackendProvider>
  );
  const sender = renderHook(() => useSend(), { wrapper });
  await act(async () => {
    await sender.result.current.send(tab.id);
  });
  expect(backend.send).toHaveBeenCalledTimes(1);
  expect(backend.send.mock.calls[0]![0]).toMatchObject({ sourceRoot: root, origin: null });
});
