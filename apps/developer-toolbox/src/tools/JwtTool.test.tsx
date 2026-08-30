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
  const token = screen.getByLabelText("JWT 컴팩트 토큰");
  const key = screen.getByLabelText("JWT 검증 키");
  fireEvent.change(token, { target: { value: TOKEN } });
  fireEvent.change(key, { target: { value: KEY } });
  return { token, key, verify: screen.getByRole("button", { name: "서명 검증" }) };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("JwtDecoder", () => {
  it("exposes accessible inputs and keeps decode visibly unverified", async () => {
    render(<JwtDecoder />);
    const token = screen.getByLabelText("JWT 컴팩트 토큰");
    const key = screen.getByLabelText("JWT 검증 키");
    expect(token.getAttribute("aria-describedby")).toBe("jwt-help");
    expect(key.getAttribute("type")).toBe("password");

    fireEvent.change(token, { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "디코드" }));

    await waitFor(() => expect(screen.getByLabelText("JWT 디코드 결과").textContent).toContain("unverified"));
    expect(screen.getByRole("status").textContent).toContain("검증되지 않음");
    expect(mocks.verifyJwt).not.toHaveBeenCalled();
    expect(screen.getByLabelText("JWT 디코드 결과").textContent).not.toContain(SIGNATURE);
  });

  it("can verify explicitly after a completed decode", async () => {
    mocks.verifyJwt.mockResolvedValueOnce(true);
    render(<JwtDecoder />);
    const { token, key } = fillForm();

    fireEvent.click(screen.getByRole("button", { name: "디코드" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("검증되지 않음"));
    fireEvent.click(screen.getByRole("button", { name: "서명 검증" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("검증됨"));
    expect(token.isConnected).toBe(true);
    expect(key.isConnected).toBe(true);
  });

  it("keeps programmatic paste within the token bound", () => {
    render(<JwtDecoder />);
    const token = screen.getByLabelText("JWT 컴팩트 토큰") as HTMLTextAreaElement;
    fireEvent.change(token, { target: { value: "x".repeat(JWT_LIMITS.maxTokenBytes + 1) } });
    expect(token.value).toBe("");

    fireEvent.change(token, {
      target: { value: "가".repeat(Math.floor(JWT_LIMITS.maxTokenBytes / 3) + 1) },
    });
    expect(token.value).toBe("");
  });

  it("performs verification only after an explicit action and reports verified state", async () => {
    mocks.verifyJwt.mockResolvedValueOnce(true);
    render(<JwtDecoder />);
    const { verify } = fillForm();

    fireEvent.click(verify);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("검증됨"));
    expect(mocks.verifyJwt).toHaveBeenCalledWith({
      algorithm: "HS256",
      signingInput: SIGNING_INPUT,
      signature: SIGNATURE,
      key: KEY,
      keyEncoding: "utf8",
    });
    expect(screen.getByLabelText("JWT 디코드 결과").textContent).toContain('"verification": "verified"');
  });

  it("blocks duplicate verification while an operation is pending", async () => {
    let resolve: (value: boolean) => void = () => undefined;
    mocks.verifyJwt.mockReturnValueOnce(new Promise<boolean>((done) => {
      resolve = done;
    }));
    render(<JwtDecoder />);
    const { verify } = fillForm();

    fireEvent.click(verify);
    fireEvent.click(screen.getByRole("button", { name: "검증 중..." }));
    expect(mocks.verifyJwt).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toContain("검증하는 중");

    resolve(true);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("검증됨"));
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
    expect(screen.getByLabelText("JWT 디코드 결과").textContent).not.toContain("verified");
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

  it("uses fixed clipboard and output action errors without reflecting platform details", async () => {
    mocks.readClipboardText.mockRejectedValueOnce(new Error("C:\\private\\secret"));
    const writeText = vi.fn().mockRejectedValue(new Error("/private/output/path"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<JwtDecoder />);

    const token = screen.getByLabelText("JWT 컴팩트 토큰");
    fireEvent.contextMenu(token, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole("menuitem", { name: "붙여넣기" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "JWT 입력을 클립보드에서 읽지 못했습니다.",
    );

    fireEvent.change(token, { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "디코드" }));
    const output = screen.getByLabelText("JWT 디코드 결과");
    await waitFor(() => expect(output.textContent).toContain("unverified"));
    fireEvent.contextMenu(output, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole("menuitem", { name: "복사" }));
    await waitFor(() => {
      expect(screen.getAllByRole("alert").some((entry) =>
        entry.textContent === "JWT 결과 작업을 완료하지 못했습니다.",
      )).toBe(true);
    });
  });
});
