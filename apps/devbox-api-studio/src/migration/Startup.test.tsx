import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MigrationStartup } from "./Startup";
const fixture = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/api-studio-features/transport", () => ({ componentInvoke: () => fixture.invoke }));
afterEach(() => { cleanup(); fixture.invoke.mockReset(); });
it("lets the native startup owner decide whether a source discovery failure can be skipped", async () => {
  let recovering = true;
  fixture.invoke.mockImplementation(async (method: string) => {
    if (method === "migration_status") throw new Error("기존 저장소를 확인하지 못했습니다.");
    if (method === "finish_startup" && recovering) throw new Error("중단된 가져오기를 먼저 복구하세요.");
    if (method === "finish_startup") return null;
    throw new Error("unexpected fixture command");
  });
  render(<MigrationStartup><p>product features mounted</p></MigrationStartup>);
  await screen.findByRole("alert");
  const open = screen.getByRole("button", { name: "가져오기 없이 계속" });
  fireEvent.click(open);
  await screen.findByText("중단된 가져오기를 먼저 복구하세요.");
  expect(screen.queryByText("product features mounted")).toBeNull();
  recovering = false; fireEvent.click(open);
  await screen.findByText("product features mounted");
});
it("reviews a supported profile subset when the legacy store exceeds one batch", async () => {
  const profiles = Array.from({ length: 65 }, (_, i) => ({ id: `profile-${i}`, bytes: 1024 }));
  fixture.invoke.mockImplementation(async (method: string) => {
    if (method === "migration_status") return { busy: false, reviewNeeded: true, pending: null, sources: [{ app: "webhook-lab", present: true, readable: true }], profiles };
    if (method === "prepare_migration") throw new Error("fixture stops before native mutation");
    throw new Error("unexpected fixture command");
  });
  render(<MigrationStartup><p>features</p></MigrationStartup>);
  await screen.findByText(/가져올 모의 서버 프로필 선택/);
  fireEvent.click(screen.getByRole("button", { name: "선택한 데이터 확인" }));
  await waitFor(() => expect(fixture.invoke).toHaveBeenCalledWith("prepare_migration", expect.objectContaining({ profileIds: profiles.slice(0, 64).map(p => p.id) })));
  expect(screen.queryByText("features")).toBeNull();
});
