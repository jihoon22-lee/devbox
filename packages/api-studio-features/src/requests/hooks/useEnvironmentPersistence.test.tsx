import { renderHook, act } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useEnvironmentPersistence } from "./useEnvironmentPersistence";
import { saveStore } from "../lib/environments";
vi.mock("../lib/environments", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/environments")>()),
  saveStore: vi.fn(),
}));
vi.mock("../api", () => ({ sealSecret: vi.fn() }));
it("does not select a newly created environment when persistence rejects it", async () => {
  vi.mocked(saveStore).mockRejectedValueOnce(new Error("synthetic save failure"));
  const select = vi.fn();
  const warning = vi.fn();
  const lock = vi.fn();
  const { result } = renderHook(() =>
    useEnvironmentPersistence({
      environmentRevisionRef: { current: 0 },
      environmentBusyRef: { current: false },
      envStoreRef: { current: { version: 1, environments: [{ id: "old", name: "dev", variables: [] }] } },
      setEnvStore: vi.fn(),
      environmentMutationBusyRef: { current: false },
      mountedRef: { current: true },
      setPersistenceReady: lock,
      setPersistenceWarning: warning,
      transferBusyRef: { current: false },
      setEnvironmentBusy: vi.fn(),
      setError: vi.fn(),
      persistenceReady: true,
      envName: "dev",
      setCurrentEnvId: select,
      setEnvName: vi.fn(),
    }),
  );
  await act(() => result.current.onCreateEnv());
  expect(select).not.toHaveBeenCalled();
  expect(lock).toHaveBeenCalledWith(false);
  expect(warning).toHaveBeenCalledWith(expect.stringContaining("앱을 다시 열어"));
});
