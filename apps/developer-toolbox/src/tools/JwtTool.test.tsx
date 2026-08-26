import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  readClipboardText: vi.fn().mockResolvedValue(""),
  verifyJwt: vi.fn(),
}));

vi.mock("../api", () => mocks);

import { JwtDecoder } from "./JwtTool";
import { JWT_LIMITS } from "./jwt";

const SIGNING_INPUT =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
const SIGNATURE = "AL_nmexgcwawKDK5uJ0RtfAxT1GguksdPuaahEACpHc";
const TOKEN = `${SIGNING_INPUT}.${SIGNATURE}`;
const KEY = "01234567890123456789012345678901";

function fillForm(): { token: HTMLElement; key: HTMLElement; verify: HTMLElement } {
  const token = screen.getByLabelText("JWT compact token");
  const key = screen.getByLabelText("JWT verification key");
  fireEvent.change(token, { target: { value: TOKEN } });
  fireEvent.change(key, { target: { value: KEY } });
  return { token, key, verify: screen.getByRole("button", { name: "Verify signature" }) };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("JwtDecoder", () => {
  it("exposes accessible inputs and keeps decode visibly unverified", async () => {
    render(<JwtDecoder />);
    const token = screen.getByLabelText("JWT compact token");
    const key = screen.getByLabelText("JWT verification key");
    expect(token.getAttribute("aria-describedby")).toBe("jwt-help");
    expect(key.getAttribute("type")).toBe("password");

    fireEvent.change(token, { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Decode" }));

    await waitFor(() => expect(screen.getByLabelText("JWT decoded output").textContent).toContain("unverified"));
    expect(screen.getByRole("status").textContent).toContain("Unverified");
    expect(mocks.verifyJwt).not.toHaveBeenCalled();
    expect(screen.getByLabelText("JWT decoded output").textContent).not.toContain(SIGNATURE);
  });

  it("can verify explicitly after a completed decode", async () => {
    mocks.verifyJwt.mockResolvedValueOnce(true);
    render(<JwtDecoder />);
    const { token, key } = fillForm();

    fireEvent.click(screen.getByRole("button", { name: "Decode" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Unverified"));
    fireEvent.click(screen.getByRole("button", { name: "Verify signature" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Verified"));
    expect(token.isConnected).toBe(true);
    expect(key.isConnected).toBe(true);
  });

  it("keeps programmatic paste within the token bound", () => {
    render(<JwtDecoder />);
    const token = screen.getByLabelText("JWT compact token") as HTMLTextAreaElement;
    fireEvent.change(token, { target: { value: "x".repeat(JWT_LIMITS.maxTokenBytes + 1) } });
    expect(token.value).toBe("");
  });

  it("performs verification only after an explicit action and reports verified state", async () => {
    mocks.verifyJwt.mockResolvedValueOnce(true);
    render(<JwtDecoder />);
    const { verify } = fillForm();

    fireEvent.click(verify);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Verified"));
    expect(mocks.verifyJwt).toHaveBeenCalledWith({
      algorithm: "HS256",
      signingInput: SIGNING_INPUT,
      signature: SIGNATURE,
      key: KEY,
      keyEncoding: "utf8",
    });
    expect(screen.getByLabelText("JWT decoded output").textContent).toContain('"verification": "verified"');
  });

  it("blocks duplicate verification while an operation is pending", async () => {
    let resolve: (value: boolean) => void = () => undefined;
    mocks.verifyJwt.mockReturnValueOnce(new Promise<boolean>((done) => {
      resolve = done;
    }));
    render(<JwtDecoder />);
    const { verify } = fillForm();

    fireEvent.click(verify);
    fireEvent.click(screen.getByRole("button", { name: "Verifying..." }));
    expect(mocks.verifyJwt).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toContain("Verifying");

    resolve(true);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Verified"));
  });

  it("ignores a late native result after unmount and remount", async () => {
    let resolve: (value: boolean) => void = () => undefined;
    mocks.verifyJwt.mockReturnValueOnce(new Promise<boolean>((done) => {
      resolve = done;
    }));
    const rendered = render(<JwtDecoder />);
    const { verify } = fillForm();
    fireEvent.click(verify);
    rendered.unmount();
    render(<JwtDecoder />);

    resolve(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(screen.getByLabelText("JWT decoded output").textContent).not.toContain("verified");
  });

  it("maps native failures to a fixed message and does not run during IME key events", async () => {
    mocks.verifyJwt.mockRejectedValueOnce(new Error("DO_NOT_REFLECT_SECRET_OR_PLATFORM"));
    render(<JwtDecoder />);
    const { token, verify } = fillForm();
    fireEvent.keyDown(token, { key: "Enter", isComposing: true });
    expect(mocks.verifyJwt).not.toHaveBeenCalled();

    fireEvent.click(verify);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("JWT 검증을 처리할 수 없습니다.");
    expect(alert.textContent).not.toContain("DO_NOT_REFLECT");
  });
});
