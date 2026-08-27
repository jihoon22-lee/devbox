import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as api from "./api";

vi.mock("./api", () => ({
  CLIPBOARD_PREVIEW_ID: "builtin/clipboard-preview",
  getShortcut: vi.fn(async () => ({ accelerator: "Ctrl+Alt+Space", enabled: true, registration: "registered", alternatives: ["Ctrl+Alt+L", "Ctrl+Alt+J"] })),
  launchResult: vi.fn(async () => ({ status: "launched", appId: "workbench" })),
  performTextAction: vi.fn(async () => ({ status: "launched", appId: "developer-toolbox" })),
  previewTextAction: vi.fn(async (actionId: string) => ({ actionId, kind: actionId === "builtin/clipboard-preview" ? "clipboard-preview/v1" : "handoff:toolbox-text/v1", maxBytes: 65536 })),
  readCurrentText: vi.fn(async () => "selected text"),
  search: vi.fn(async () => ({
    results: [
      { id: "catalog/app/workbench", label: "Workbench", detail: "Devbox 앱", source: "catalog", targetApp: "workbench", targetKind: "app", stale: false, explicitPreview: false },
      { id: "builtin/clipboard-preview", label: "Clipboard 미리보기", detail: "현재 선택 영역, 없으면 clipboard · 전달하지 않음", source: "launcher", targetApp: "devbox-launcher", targetKind: "clipboard-preview", stale: false, explicitPreview: true },
    ],
    sources: [],
  })),
  setShortcut: vi.fn(async (config) => config),
}));

describe("Devbox Launcher", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(() => cleanup());

  it("loads catalog results without reading clipboard", async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByRole("option", { name: /Workbench/ })).toBeInTheDocument());
    expect(api.readCurrentText).not.toHaveBeenCalled();
  });

  it("only samples selected text for the explicit clipboard preview fallback", async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByRole("option", { name: /Clipboard 미리보기/ })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("option", { name: /^Clipboard 미리보기/ }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole("button", { name: "닫기" })).toHaveFocus());
    expect(api.readCurrentText).toHaveBeenCalledTimes(1);
    expect(api.performTextAction).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toHaveTextContent("selected text");
  });

  it("does not treat an IME composing Enter as an action", async () => {
    render(<App />);
    const input = await screen.findByRole("combobox", { name: "열기 또는 검색" });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229, isComposing: true });
    expect(api.launchResult).not.toHaveBeenCalled();
  });

  it("explains alternative controls when the global shortcut is unavailable", async () => {
    vi.mocked(api.getShortcut).mockResolvedValue({
      accelerator: "Ctrl+Alt+Space",
      enabled: true,
      registration: "unavailable",
      alternatives: ["Ctrl+Alt+L", "Ctrl+Alt+J"],
    });
    render(<App />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Ctrl+Alt+L 또는 Ctrl+Alt+J"));
    expect(screen.getByRole("combobox", { name: "Launcher 단축키" })).toBeInTheDocument();
  });

  it("applies an allow-listed replacement shortcut immediately", async () => {
    vi.mocked(api.setShortcut).mockResolvedValue({
      accelerator: "Ctrl+Alt+L",
      enabled: true,
      registration: "registered",
      alternatives: ["Ctrl+Alt+Space", "Ctrl+Alt+J"],
    });
    render(<App />);
    const select = await screen.findByRole("combobox", { name: "Launcher 단축키" });
    fireEvent.change(select, { target: { value: "Ctrl+Alt+L" } });
    await waitFor(() => expect(api.setShortcut).toHaveBeenCalledWith({
      accelerator: "Ctrl+Alt+L",
      enabled: true,
    }));
    expect(screen.getByText("즉시 적용")).toBeInTheDocument();
  });

  it("uses an accessible confirmation dialog before opening stale results", async () => {
    vi.mocked(api.search).mockResolvedValue({
      results: [
        { id: "snapshot/workbench/old", label: "Old profile", detail: "Workbench", source: "workbench", targetApp: "workbench", targetKind: "profile", stale: true, explicitPreview: false },
      ],
      sources: [{ producer: "workbench", view: "profiles", status: "stale" }],
    });
    render(<App />);
    const result = await screen.findByRole("option", { name: /Old profile/ });
    fireEvent.click(result);
    const dialog = await screen.findByRole("dialog", { name: "오래된 snapshot입니다" });
    const cancel = screen.getByRole("button", { name: "취소" });
    const continueButton = screen.getByRole("button", { name: "계속 열기" });
    await waitFor(() => expect(cancel).toHaveFocus());
    continueButton.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(continueButton).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.launchResult).not.toHaveBeenCalled();
  });

  it("revalidates and launches a stale result only after confirmation", async () => {
    vi.mocked(api.search).mockResolvedValue({
      results: [
        { id: "snapshot/workbench/old", label: "Old profile", detail: "Workbench", source: "workbench", targetApp: "workbench", targetKind: "profile", stale: true, explicitPreview: false },
      ],
      sources: [{ producer: "workbench", view: "profiles", status: "stale" }],
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("option", { name: /Old profile/ }));
    fireEvent.click(await screen.findByRole("button", { name: "계속 열기" }));
    await waitFor(() => expect(api.launchResult).toHaveBeenCalledWith("snapshot/workbench/old"));
  });
});
