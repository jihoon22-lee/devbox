import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Startup } from "./Startup";
const { rpc } = vi.hoisted(() => ({ rpc: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => rpc }));
beforeEach(() => {
  rpc.mockReset();
});
afterEach(cleanup);
it("does not mount a domain before automatic preparation completes", async () => {
  let finish!: (value: { active: boolean }) => void;
  rpc.mockResolvedValueOnce({ active: false }).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(
    <Startup>
      <p>domain mounted</p>
    </Startup>,
  );
  await vi.waitFor(() => expect(rpc).toHaveBeenCalledWith("start_empty", {}));
  expect(screen.queryByText("domain mounted")).toBeNull();
  finish({ active: true });
  await screen.findByText("domain mounted");
});
it("preserves a future store without offering a reset or mounting features", async () => {
  const error = new Error("더 최신 버전의 저장소입니다. 원본을 유지했습니다.");
  error.name = "future_schema";
  rpc.mockRejectedValue(error);
  render(
    <Startup>
      <p>domain mounted</p>
    </Startup>,
  );
  await screen.findByRole("alert");
  for (const button of screen.getAllByRole("button")) {
    expect(button.hasAttribute("disabled")).toBe(true);
    fireEvent.click(button);
  }
  expect(rpc).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("domain mounted")).toBeNull();
});

it("can review a replacement folder for an unavailable binding without starting domains", async () => {
  rpc.mockImplementation(async (method: string, args: { path?: string } = {}) => {
    if (method === "status") return { active: false, hasExisting: true, bindingUnavailable: true };
    if (method === "schedule_vault_change")
      return { schedule: { id: "native-plan", target: args.path, previousRoot: "C:/unavailable" } };
    return { schedule: null };
  });
  render(
    <Startup>
      <p>domain mounted</p>
    </Startup>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "다른 노트 폴더 선택" }));
  await screen.findByRole("heading", { name: "노트 폴더 연결" });
  const input = screen.getByRole("textbox", { name: "연결할 노트 폴더" });
  await vi.waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
  fireEvent.change(input, { target: { value: "C:/replacement" } });
  fireEvent.click(screen.getByRole("button", { name: "선택한 폴더 확인" }));
  await screen.findByRole("heading", { name: "노트 폴더 변경 확인" });
  expect(screen.queryByText("domain mounted")).toBeNull();
  expect(rpc.mock.calls.some(([method]) => method === "start_empty" || method === "continue_existing")).toBe(false);
});

it("reads only local recovery on fresh offline startup and keeps source mutations unavailable", async () => {
  rpc.mockImplementation(async (method: string) =>
    method === "status"
      ? { active: false, hasExisting: true, bindingUnavailable: true }
      : { entries: [{ path: "a.md", content: "LOCAL_ONLY", baseRevision: "r", savedAtMs: 1 }], otherVaultCount: 1 },
  );
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
  render(
    <Startup>
      <p>domain mounted</p>
    </Startup>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "로컬 복구본 확인" }));
  await screen.findByText("LOCAL_ONLY");
  fireEvent.click(screen.getByRole("button", { name: "복구본 복사" }));
  expect(copy).toHaveBeenCalledWith("LOCAL_ONLY");
  expect(rpc).toHaveBeenCalledWith("load_recovery", {});
  expect(screen.queryByText("domain mounted")).toBeNull();
  expect(screen.queryByRole("button", { name: "삭제된 노트 재생성" })).toBeNull();
});
