import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createApiRequestHandoff } from "../api";
import { ApiHandoffAction } from "./ApiHandoffAction";

vi.mock("../api", () => ({
  createApiRequestHandoff: vi.fn(),
}));

const createApiRequestHandoffMock = vi.mocked(createApiRequestHandoff);

beforeEach(() => {
  createApiRequestHandoffMock.mockReset().mockResolvedValue({
    handoffId: "0123456789abcdef0123456789abcdef",
    producerId: "developer-toolbox",
    consumerId: "api-playground",
    createdAtMs: 1_700_000_000_000,
    expiresAtMs: 1_700_000_600_000,
  });
});

afterEach(() => cleanup());

describe("API Playground output handoff", () => {
  it("previews and edits the current output before a manual one-time publish", async () => {
    render(<ApiHandoffAction value="current output" />);

    fireEvent.click(screen.getByRole("button", { name: "API Playground로 보내기" }));
    const dialog = screen.getByRole("dialog", { name: "API Playground 요청 미리보기" });
    expect(dialog.textContent).toContain("POST");
    expect(dialog.textContent).toContain("/");
    expect(dialog.textContent).toContain("text/plain; charset=utf-8");
    expect(createApiRequestHandoffMock).not.toHaveBeenCalled();

    fireEvent.change(screen.getByRole("textbox", { name: "API Playground request body" }), {
      target: { value: "edited output" },
    });
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 전달" }));

    await waitFor(() => expect(createApiRequestHandoffMock).toHaveBeenCalledWith("edited output"));
    expect(screen.queryByRole("dialog", { name: "API Playground 요청 미리보기" })).toBeNull();
    expect(screen.getByRole("status").textContent).toContain(
      "developer-toolbox → api-playground",
    );
    expect(screen.getByRole("status").textContent).not.toContain(
      "0123456789abcdef0123456789abcdef",
    );
  });

  it("cancels without publishing or using the clipboard", () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<ApiHandoffAction value="cancelled output" />);

    fireEvent.click(screen.getByRole("button", { name: "API Playground로 보내기" }));
    fireEvent.click(screen.getByRole("button", { name: "취소" }));

    expect(createApiRequestHandoffMock).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "API Playground 요청 미리보기" })).toBeNull();
  });

  it("rejects malformed edited output with a fixed error", () => {
    render(<ApiHandoffAction value="safe output" />);
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 보내기" }));
    fireEvent.change(screen.getByRole("textbox", { name: "API Playground request body" }), {
      target: { value: "unsafe\0output" },
    });
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 전달" }));

    expect(screen.getByRole("alert").textContent).toBe(
      "API Playground로 전달할 텍스트가 유효하지 않습니다",
    );
    expect(createApiRequestHandoffMock).not.toHaveBeenCalled();
  });

  it("rejects an unpaired surrogate without invoking the native producer", () => {
    render(<ApiHandoffAction value="safe output" />);
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 보내기" }));
    fireEvent.change(screen.getByRole("textbox", { name: "API Playground request body" }), {
      target: { value: "unsafe\ud800output" },
    });
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 전달" }));

    expect(screen.getByRole("alert").textContent).toBe(
      "API Playground로 전달할 텍스트가 유효하지 않습니다",
    );
    expect(createApiRequestHandoffMock).not.toHaveBeenCalled();
  });

  it("does not echo an unexpected native error or output to the clipboard", async () => {
    createApiRequestHandoffMock.mockRejectedValueOnce(new Error("/private/output/path"));
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<ApiHandoffAction value="safe output" />);

    fireEvent.click(screen.getByRole("button", { name: "API Playground로 보내기" }));
    fireEvent.click(screen.getByRole("button", { name: "API Playground로 전달" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다",
    );
    expect(screen.queryByText("/private/output/path")).toBeNull();
    expect(writeText).not.toHaveBeenCalled();
  });
});
