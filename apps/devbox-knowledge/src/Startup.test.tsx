import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Startup } from "./Startup";
const { rpc } = vi.hoisted(() => ({ rpc: vi.fn() }));
vi.mock("@devbox/product-shell/api", () => ({ nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => rpc }));
beforeEach(() => { rpc.mockReset(); });
afterEach(cleanup);
it("does not mount any domain until the explicit startup action succeeds", async () => {
  rpc.mockResolvedValueOnce({ active: false }).mockResolvedValueOnce({ active: true });
  render(<Startup><p>domain mounted</p></Startup>);
  const button = await screen.findByRole("button", { name: "새 저장소로 시작" });
  await vi.waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
  expect(screen.queryByText("domain mounted")).toBeNull();
  expect(rpc.mock.calls.map(call => call[0])).toEqual(["status"]);
  fireEvent.click(button);
  await screen.findByText("domain mounted");
  expect(rpc.mock.calls.map(call => call[0])).toEqual(["status", "start_empty"]);
});
it("preserves a future store without offering a reset or mounting features", async () => {
  const error = new Error("더 최신 버전의 저장소입니다. 원본을 유지했습니다."); error.name = "future_schema";
  rpc.mockRejectedValue(error);
  render(<Startup><p>domain mounted</p></Startup>);
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
    if (method === "schedule_vault_change") return { schedule: { id: "native-plan", target: args.path, previousRoot: "C:/unavailable" } };
    return { schedule: null };
  });
  render(<Startup><p>domain mounted</p></Startup>);
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
