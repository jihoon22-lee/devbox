import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IDENTIFIER_GENERATION_ERROR } from "./ids";

const mocks = vi.hoisted(() => ({
  generateIds: vi.fn(),
  hash: vi.fn(),
  readClipboardText: vi.fn(),
}));

vi.mock("../api", () => mocks);

import { UuidTool } from "./security";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("UuidTool", () => {
  beforeEach(() => {
    mocks.generateIds.mockResolvedValue(["safe-result"]);
  });

  it("locks generation options while an IPC request is pending", async () => {
    let resolve: (value: string[]) => void = () => undefined;
    mocks.generateIds.mockReturnValueOnce(new Promise<string[]>((done) => {
      resolve = done;
    }));

    render(<UuidTool />);
    fireEvent.click(screen.getByRole("button", { name: "생성" }));
    fireEvent.click(screen.getByRole("button", { name: "생성 중..." }));

    expect(screen.getByRole("status").textContent).toContain("생성하는 중");
    expect(mocks.generateIds).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "생성 중..." })).toBeTruthy();
    expect((screen.getByLabelText("식별자 종류") as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByLabelText("생성 수량") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("대문자 출력") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("하이픈 표시") as HTMLInputElement).disabled).toBe(true);

    resolve(["first-result"]);
    await waitFor(() => expect(screen.getByLabelText("생성된 식별자 출력").textContent).toContain("first-result"));
    expect(screen.getByRole("status").textContent).toContain("1개 식별자");
  });

  it("does not submit while the count field is composing with an IME", async () => {
    render(<UuidTool />);
    const count = screen.getByLabelText("생성 수량");
    const button = screen.getByRole("button", { name: "생성" });

    fireEvent.compositionStart(count);
    fireEvent.click(button);
    expect(mocks.generateIds).not.toHaveBeenCalled();

    fireEvent.compositionEnd(count);
    fireEvent.click(button);
    await waitFor(() => expect(mocks.generateIds).toHaveBeenCalledTimes(1));
  });

  it("ignores a late response after the tool is unmounted", async () => {
    let resolve: (value: string[]) => void = () => undefined;
    mocks.generateIds.mockReturnValueOnce(new Promise<string[]>((done) => {
      resolve = done;
    }));

    const rendered = render(<UuidTool />);
    fireEvent.click(screen.getByRole("button", { name: "생성" }));
    rendered.unmount();
    render(<UuidTool />);

    resolve(["late-result"]);
    await Promise.resolve();
    await Promise.resolve();
    expect(screen.getByLabelText("생성된 식별자 출력").textContent).not.toContain("late-result");
  });

  it("does not reflect raw native or browser errors", async () => {
    mocks.generateIds.mockRejectedValueOnce(new Error("DO_NOT_REFLECT_PLATFORM_DETAIL"));
    render(<UuidTool />);

    fireEvent.click(screen.getByRole("button", { name: "생성" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(IDENTIFIER_GENERATION_ERROR);
    expect(alert.textContent).not.toContain("DO_NOT_REFLECT_PLATFORM_DETAIL");
  });
});
