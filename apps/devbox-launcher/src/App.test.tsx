import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";
import * as api from "./api";

vi.mock("./api", () => ({
  CLIPBOARD_PREVIEW_ID: "builtin/clipboard-preview",
  clearRecents: vi.fn(async () => undefined),
  getShortcut: vi.fn(async () => ({ accelerator: "Ctrl+Alt+Space", enabled: true, registration: "registered", alternatives: ["Ctrl+Alt+L", "Ctrl+Alt+J"] })),
  launchResult: vi.fn(async () => ({ status: "launched", appId: "workbench" })),
  performTextAction: vi.fn(async () => ({ status: "launched", appId: "developer-toolbox" })),
  previewTextAction: vi.fn(async (result: { id: string }) => ({ actionId: result.id, kind: result.id === "builtin/clipboard-preview" ? "clipboard-preview/v1" : "handoff:toolbox-text/v1", maxBytes: 65536 })),
  readCurrentText: vi.fn(async () => "selected text"),
  search: vi.fn(async () => ({
    results: [
      { id: "catalog/app/workbench", revision: "a".repeat(64), label: "Workbench", detail: "Devbox 앱", source: "catalog", targetApp: "workbench", targetKind: "app", stale: false, explicitPreview: false, favorite: false, recent: false },
      { id: "builtin/clipboard-preview", revision: "b".repeat(64), label: "클립보드 미리보기", detail: "현재 선택 영역, 없으면 클립보드 · 전달하지 않음", source: "launcher", targetApp: "devbox-launcher", targetKind: "clipboard-preview", stale: false, explicitPreview: true, favorite: false, recent: false },
    ],
    sources: [],
  })),
  setFavorite: vi.fn(async () => undefined),
  setShortcut: vi.fn(async (config) => config),
}));

const DEFAULT_SEARCH_RESPONSE = {
  results: [
    { id: "catalog/app/workbench", revision: "a".repeat(64), label: "Workbench", detail: "Devbox 앱", source: "catalog", targetApp: "workbench", targetKind: "app", stale: false, explicitPreview: false, favorite: false, recent: false },
    { id: "builtin/clipboard-preview", revision: "b".repeat(64), label: "클립보드 미리보기", detail: "현재 선택 영역, 없으면 클립보드 · 전달하지 않음", source: "launcher", targetApp: "devbox-launcher", targetKind: "clipboard-preview", stale: false, explicitPreview: true, favorite: false, recent: false },
  ],
  sources: [],
};

describe("Devbox Launcher", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.search).mockResolvedValue(DEFAULT_SEARCH_RESPONSE);
  });
  afterEach(() => cleanup());

  it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
    const { container } = render(<App />);
    await screen.findByRole("option", { name: /Workbench/ });
    await assertNoA11yViolations(container);
  });

  it("loads catalog results without reading clipboard", async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByRole("option", { name: /Workbench/ })).toBeInTheDocument());
    expect(api.readCurrentText).not.toHaveBeenCalled();
  });

  it("passes the result revision and a strict fresh-launch flag", async () => {
    render(<App />);
    const result = await screen.findByRole("option", { name: /Workbench/ });
    fireEvent.click(result);
    await waitFor(() => expect(api.launchResult).toHaveBeenCalledWith(expect.objectContaining({
      id: "catalog/app/workbench",
      revision: expect.stringMatching(/^[0-9a-f]{64}$/),
    }), false));
  });

  it("only samples selected text for the explicit clipboard preview fallback", async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByRole("option", { name: /클립보드 미리보기/ })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("option", { name: /^클립보드 미리보기/ }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole("button", { name: "닫기" })).toHaveFocus());
    expect(api.readCurrentText).toHaveBeenCalledTimes(1);
    expect(api.performTextAction).not.toHaveBeenCalled();
    expect(api.previewTextAction).toHaveBeenCalledWith(expect.objectContaining({
      id: "builtin/clipboard-preview",
      revision: "b".repeat(64),
    }));
    expect(screen.getByRole("dialog")).toHaveTextContent("selected text");
  });

  it("carries the result revision through preview and confirmed text handoff", async () => {
    vi.mocked(api.search).mockResolvedValue({
      results: [
        { id: "catalog/action/developer-toolbox/transform-text", revision: "d".repeat(64), label: "텍스트 변환", detail: "명시적 미리보기", source: "catalog", targetApp: "developer-toolbox", targetKind: "handoff", stale: false, explicitPreview: true, favorite: false, recent: false },
      ],
      sources: [],
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("option", { name: /텍스트 변환/ }));
    fireEvent.click(await screen.findByRole("button", { name: "전달" }));
    await waitFor(() => expect(api.performTextAction).toHaveBeenCalledWith(expect.objectContaining({
      id: "catalog/action/developer-toolbox/transform-text",
      revision: "d".repeat(64),
    }), "selected text"));
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
        { id: "snapshot/workbench/old", revision: "c".repeat(64), label: "Old profile", detail: "Workbench", source: "workbench", targetApp: "workbench", targetKind: "profile", stale: true, explicitPreview: false, favorite: false, recent: false },
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
    fireEvent.keyDown(dialog, { key: "Escape", keyCode: 229, isComposing: true });
    expect(screen.getByRole("dialog", { name: "오래된 snapshot입니다" })).toBeInTheDocument();
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.launchResult).not.toHaveBeenCalled();
  });

  it("revalidates and launches a stale result only after confirmation", async () => {
    vi.mocked(api.search).mockResolvedValue({
      results: [
        { id: "snapshot/workbench/old", revision: "c".repeat(64), label: "Old profile", detail: "Workbench", source: "workbench", targetApp: "workbench", targetKind: "profile", stale: true, explicitPreview: false, favorite: false, recent: false },
      ],
      sources: [{ producer: "workbench", view: "profiles", status: "stale" }],
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("option", { name: /Old profile/ }));
    fireEvent.click(await screen.findByRole("button", { name: "계속 열기" }));
    await waitFor(() => expect(api.launchResult).toHaveBeenCalledWith(expect.objectContaining({
      id: "snapshot/workbench/old",
      revision: "c".repeat(64),
    }), true));
  });

  it("keeps favorite toggles separate from result execution and refreshes the query", async () => {
    render(<App />);
    await screen.findByRole("option", { name: /Workbench/ });
    fireEvent.click(screen.getByRole("button", { name: "Workbench 즐겨찾기 추가" }));
    await waitFor(() => expect(api.setFavorite).toHaveBeenCalledWith(expect.objectContaining({
      id: "catalog/app/workbench",
      revision: "a".repeat(64),
    }), true));
    expect(api.launchResult).not.toHaveBeenCalled();
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(2));
  });

  it("clears recent history and refreshes the current query", async () => {
    render(<App />);
    await screen.findByRole("option", { name: /Workbench/ });
    fireEvent.click(screen.getByRole("button", { name: "최근 기록 초기화" }));
    await waitFor(() => expect(api.clearRecents).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(2));
  });

  it("renders friendly Korean source status labels and explanations", async () => {
    vi.mocked(api.search).mockResolvedValue({
      results: [],
      sources: [
        { producer: "workbench", view: "profiles", status: "fresh" },
        { producer: "repo-manager", view: "repositories", status: "stale" },
        { producer: "run-manager", view: "jobs-services", status: "missing" },
        { producer: "everything-plus", view: "saved-queries", status: "corrupt" },
        { producer: "wsl-desktop", view: "profiles", status: "permission" },
        { producer: "unknown-source", view: "profiles", status: "linked" },
      ],
    });
    render(<App />);
    fireEvent.click(await screen.findByText("snapshot source 상태"));
    expect(screen.getByText("최신")).toBeInTheDocument();
    expect(screen.getByText("오래됨")).toBeInTheDocument();
    expect(screen.getByText("없음")).toBeInTheDocument();
    expect(screen.getByText("손상됨")).toBeInTheDocument();
    expect(screen.getByText("권한 없음")).toBeInTheDocument();
    expect(screen.getByText("안전하지 않은 링크")).toBeInTheDocument();
    expect(screen.getByText("정상적으로 읽었습니다.")).toBeInTheDocument();
    expect(screen.getByText("안전하지 않아 검색에서 제외했습니다.")).toBeInTheDocument();
  });
});
