import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HMAC_ERROR } from "./hmac";

const mocks = vi.hoisted(() => ({
  hmacGenerate: vi.fn(),
  hmacVerify: vi.fn(),
  readClipboardText: vi.fn(),
}));

vi.mock("../api", () => mocks);

import { HmacTool } from "./HmacTool";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("HmacTool", () => {
  beforeEach(() => {
    mocks.hmacGenerate.mockResolvedValue("generated-tag");
    mocks.hmacVerify.mockResolvedValue(true);
  });

  function fillGenerateInputs() {
    fireEvent.change(screen.getByLabelText("HMAC 키"), { target: { value: "secret" } });
    fireEvent.change(screen.getByLabelText("HMAC 메시지"), { target: { value: "payload" } });
  }

  it("sends the explicit algorithm and encoding wire contract and exposes a copyable result", async () => {
    render(<HmacTool />);
    fillGenerateInputs();

    fireEvent.click(screen.getByRole("button", { name: "HMAC 생성" }));
    await waitFor(() => expect(screen.getByLabelText("HMAC 출력").textContent).toContain("generated-tag"));
    expect(mocks.hmacGenerate).toHaveBeenCalledWith({
      algorithm: "sha256",
      key: "secret",
      keyEncoding: "utf8",
      message: "payload",
      messageEncoding: "utf8",
      outputEncoding: "hex",
    });
    expect(screen.getByRole("button", { name: "복사" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("생성했습니다");
  });

  it("locks fields and ignores a double action while an operation is pending", async () => {
    let resolve: (value: string) => void = () => undefined;
    mocks.hmacGenerate.mockReturnValueOnce(new Promise<string>((done) => {
      resolve = done;
    }));
    render(<HmacTool />);
    fillGenerateInputs();

    const button = screen.getByRole("button", { name: "HMAC 생성" });
    fireEvent.click(button);
    fireEvent.click(button);

    expect(mocks.hmacGenerate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "생성 중..." })).toBeTruthy();
    expect((screen.getByLabelText("HMAC 키") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("HMAC 메시지") as HTMLTextAreaElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("생성하는 중");

    resolve("pending-tag");
    await waitFor(() => expect(screen.getByLabelText("HMAC 출력").textContent).toContain("pending-tag"));
  });

  it("does not submit while a key is being composed with an IME", async () => {
    render(<HmacTool />);
    const key = screen.getByLabelText("HMAC 키");
    fireEvent.change(key, { target: { value: "secret" } });
    fireEvent.change(screen.getByLabelText("HMAC 메시지"), { target: { value: "payload" } });
    fireEvent.compositionStart(key);
    fireEvent.click(screen.getByRole("button", { name: "HMAC 생성" }));
    expect(mocks.hmacGenerate).not.toHaveBeenCalled();

    fireEvent.compositionEnd(key);
    fireEvent.click(screen.getByRole("button", { name: "HMAC 생성" }));
    await waitFor(() => expect(mocks.hmacGenerate).toHaveBeenCalledTimes(1));
  });

  it("ignores a late response after unmount and remount", async () => {
    let resolve: (value: string) => void = () => undefined;
    mocks.hmacGenerate.mockReturnValueOnce(new Promise<string>((done) => {
      resolve = done;
    }));
    const rendered = render(<HmacTool />);
    fillGenerateInputs();
    fireEvent.click(screen.getByRole("button", { name: "HMAC 생성" }));
    rendered.unmount();
    render(<HmacTool />);

    resolve("late-secret-tag");
    await Promise.resolve();
    await Promise.resolve();
    expect(screen.getByLabelText("HMAC 출력").textContent).not.toContain("late-secret-tag");
  });

  it("returns only a validity message in verify mode", async () => {
    render(<HmacTool />);
    fireEvent.change(screen.getByLabelText("HMAC 작업"), { target: { value: "verify" } });
    fillGenerateInputs();
    fireEvent.change(screen.getByLabelText("예상 HMAC 태그"), { target: { value: "tag" } });
    fireEvent.click(screen.getByRole("button", { name: "HMAC 검증" }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("일치합니다"));
    expect(mocks.hmacVerify).toHaveBeenCalledWith({
      algorithm: "sha256",
      key: "secret",
      keyEncoding: "utf8",
      message: "payload",
      messageEncoding: "utf8",
      outputEncoding: "hex",
      expectedTag: "tag",
    });
    expect(screen.queryByRole("button", { name: "복사" })).toBeNull();
  });

  it("uses one fixed error for malformed input and native failures", async () => {
    mocks.hmacGenerate.mockRejectedValueOnce(new Error("RAW_PLATFORM_SECRET"));
    render(<HmacTool />);
    fillGenerateInputs();
    fireEvent.click(screen.getByRole("button", { name: "HMAC 생성" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(HMAC_ERROR);
    expect(alert.textContent).not.toContain("RAW_PLATFORM_SECRET");
  });
});
