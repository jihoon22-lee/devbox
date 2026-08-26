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
    fireEvent.change(screen.getByLabelText("HMAC key"), { target: { value: "secret" } });
    fireEvent.change(screen.getByLabelText("HMAC message"), { target: { value: "payload" } });
  }

  it("sends the explicit algorithm and encoding wire contract and exposes a copyable result", async () => {
    render(<HmacTool />);
    fillGenerateInputs();

    fireEvent.click(screen.getByRole("button", { name: "Generate HMAC" }));
    await waitFor(() => expect(screen.getByLabelText("HMAC output").textContent).toContain("generated-tag"));
    expect(mocks.hmacGenerate).toHaveBeenCalledWith({
      algorithm: "sha256",
      key: "secret",
      keyEncoding: "utf8",
      message: "payload",
      messageEncoding: "utf8",
      outputEncoding: "hex",
    });
    expect(screen.getByRole("button", { name: "Copy" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("생성했습니다");
  });

  it("locks fields and ignores a double action while an operation is pending", async () => {
    let resolve: (value: string) => void = () => undefined;
    mocks.hmacGenerate.mockReturnValueOnce(new Promise<string>((done) => {
      resolve = done;
    }));
    render(<HmacTool />);
    fillGenerateInputs();

    const button = screen.getByRole("button", { name: "Generate HMAC" });
    fireEvent.click(button);
    fireEvent.click(button);

    expect(mocks.hmacGenerate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Generating..." })).toBeTruthy();
    expect((screen.getByLabelText("HMAC key") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByLabelText("HMAC message") as HTMLTextAreaElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("생성하는 중");

    resolve("pending-tag");
    await waitFor(() => expect(screen.getByLabelText("HMAC output").textContent).toContain("pending-tag"));
  });

  it("does not submit while a key is being composed with an IME", async () => {
    render(<HmacTool />);
    const key = screen.getByLabelText("HMAC key");
    fireEvent.change(key, { target: { value: "secret" } });
    fireEvent.change(screen.getByLabelText("HMAC message"), { target: { value: "payload" } });
    fireEvent.compositionStart(key);
    fireEvent.click(screen.getByRole("button", { name: "Generate HMAC" }));
    expect(mocks.hmacGenerate).not.toHaveBeenCalled();

    fireEvent.compositionEnd(key);
    fireEvent.click(screen.getByRole("button", { name: "Generate HMAC" }));
    await waitFor(() => expect(mocks.hmacGenerate).toHaveBeenCalledTimes(1));
  });

  it("ignores a late response after unmount and remount", async () => {
    let resolve: (value: string) => void = () => undefined;
    mocks.hmacGenerate.mockReturnValueOnce(new Promise<string>((done) => {
      resolve = done;
    }));
    const rendered = render(<HmacTool />);
    fillGenerateInputs();
    fireEvent.click(screen.getByRole("button", { name: "Generate HMAC" }));
    rendered.unmount();
    render(<HmacTool />);

    resolve("late-secret-tag");
    await Promise.resolve();
    await Promise.resolve();
    expect(screen.getByLabelText("HMAC output").textContent).not.toContain("late-secret-tag");
  });

  it("returns only a validity message in verify mode", async () => {
    render(<HmacTool />);
    fireEvent.change(screen.getByLabelText("HMAC operation"), { target: { value: "verify" } });
    fillGenerateInputs();
    fireEvent.change(screen.getByLabelText("Expected HMAC tag"), { target: { value: "tag" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify HMAC" }));

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
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("uses one fixed error for malformed input and native failures", async () => {
    mocks.hmacGenerate.mockRejectedValueOnce(new Error("RAW_PLATFORM_SECRET"));
    render(<HmacTool />);
    fillGenerateInputs();
    fireEvent.click(screen.getByRole("button", { name: "Generate HMAC" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(HMAC_ERROR);
    expect(alert.textContent).not.toContain("RAW_PLATFORM_SECRET");
  });
});
