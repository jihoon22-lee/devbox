import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { CaptureWindow } from "./CaptureWindow";
import { DRAFT_KEY, hideCapture } from "./capture";

vi.mock("./capture", async (original) => ({
  ...(await original<typeof import("./capture")>()),
  hideCapture: vi.fn(async () => {}),
}));
beforeEach(() => {
  sessionStorage.clear();
  vi.mocked(hideCapture).mockClear();
});
afterEach(cleanup);

test("IME and Shift+Enter do not send, Esc hides while preserving the draft", async () => {
  const wire = new NotesWire();
  render(
    <RpcProvider client={wire.client}>
      <CaptureWindow />
    </RpcProvider>,
  );
  const box = screen.getByRole("textbox", { name: "캡처할 내용" });
  fireEvent.change(box, { target: { value: "한글 draft" } });
  fireEvent.keyDown(box, { key: "Enter", isComposing: true });
  fireEvent.keyDown(box, { key: "Enter", keyCode: 229 });
  fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
  expect(wire.requests("notes.append")).toHaveLength(0);
  fireEvent.keyDown(box, { key: "Escape" });
  expect(sessionStorage.getItem(DRAFT_KEY)).toBe("한글 draft");
  expect(hideCapture).toHaveBeenCalledTimes(1);
});
test("pending Enter is deduplicated and successful capture clears draft before hiding", async () => {
  const wire = new NotesWire();
  let finish!: (value: unknown) => void;
  wire.handlers["notes.append"] = () =>
    new Promise((r) => {
      finish = r;
    });
  render(
    <RpcProvider client={wire.client}>
      <CaptureWindow />
    </RpcProvider>,
  );
  const box = screen.getByRole("textbox", { name: "캡처할 내용" });
  fireEvent.change(box, { target: { value: "owned capture" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "저장" })).toBeEnabled());
  fireEvent.keyDown(box, { key: "Enter" });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(wire.requests("notes.append")).toHaveLength(1));
  expect(hideCapture).not.toHaveBeenCalled();
  await act(async () => finish({ path: "inbox.md", rev: {} }));
  expect(hideCapture).toHaveBeenCalledTimes(1);
  expect(sessionStorage.getItem(DRAFT_KEY)).toBeNull();
  expect(box).toHaveValue("");
});
test("failed capture stays visible with its draft and an error", async () => {
  const wire = new NotesWire();
  wire.handlers["notes.append"] = () => {
    throw new RpcError("notes.target_unsaved");
  };
  sessionStorage.setItem(DRAFT_KEY, "keep draft");
  render(
    <RpcProvider client={wire.client}>
      <CaptureWindow />
    </RpcProvider>,
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "저장" })).toBeEnabled());
  fireEvent.keyDown(screen.getByRole("textbox", { name: "캡처할 내용" }), { key: "Enter" });
  expect(await screen.findByRole("alert")).toHaveTextContent(/./);
  expect(hideCapture).not.toHaveBeenCalled();
  expect(sessionStorage.getItem(DRAFT_KEY)).toBe("keep draft");
});

test("Esc keeps unsaved contents visible when sessionStorage cannot preserve the draft", async () => {
  const wire = new NotesWire();
  render(
    <RpcProvider client={wire.client}>
      <CaptureWindow />
    </RpcProvider>,
  );
  const box = screen.getByRole("textbox", { name: "캡처할 내용" });
  fireEvent.change(box, { target: { value: "unsaved owned draft" } });
  const failed = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  try {
    fireEvent.keyDown(box, { key: "Escape" });
    expect(await screen.findByRole("alert")).toHaveTextContent("임시 내용을 보관하지 못했습니다");
    expect(hideCapture).not.toHaveBeenCalled();
    expect(box).toHaveValue("unsaved owned draft");
  } finally {
    failed.mockRestore();
  }
});
