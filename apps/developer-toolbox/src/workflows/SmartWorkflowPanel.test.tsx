import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WORKFLOW_STORAGE_KEY } from "./workflowStore";
import { SmartWorkflowPanel } from "./SmartWorkflowPanel";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(),
}));

const openTool = vi.fn();

beforeEach(() => {
  localStorage.removeItem(WORKFLOW_STORAGE_KEY);
  openTool.mockReset();
});

afterEach(() => {
  cleanup();
  localStorage.removeItem(WORKFLOW_STORAGE_KEY);
});

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "Smart workflow input" }) as HTMLTextAreaElement;
}

describe("SmartWorkflowPanel", () => {
  it("shows a detection candidate and runs its selected typed stage explicitly", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: '{"name":"Ada"}' } });

    expect(screen.getByText("JSON Formatter")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    expect(screen.getByText("현재 출력 형식: JSON")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "파이프라인 실행" }));

    expect(screen.getByLabelText("Pipeline output").textContent).toContain('"name": "Ada"');
    expect(screen.getByText(/입력·출력은 저장하지 않으며/)).toBeTruthy();
  });

  it("does not auto-select an ambiguous Base64 representation", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: "Zm9v" } });

    expect(screen.getByText("여러 형식이 가능하므로 추천을 자동 선택하지 않았습니다.")).toBeTruthy();
    expect(screen.getByText("Base64 Decoder")).toBeTruthy();
    expect(screen.getByText("Base64URL Decoder")).toBeTruthy();
    expect((screen.getByRole("button", { name: "파이프라인 실행" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("persists a restartable pipeline and favorite metadata without the draft text", async () => {
    const view = render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: '{"password":"secret-value"}' } });
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    await waitFor(() => expect((screen.getByRole("button", { name: "파이프라인 저장" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
    await waitFor(() => expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "현재 도구 즐겨찾기" }));

    await waitFor(() => {
      const saved = localStorage.getItem(WORKFLOW_STORAGE_KEY) ?? "";
      expect(saved).toContain("pipeline-1");
      expect(saved).toContain("json-format");
      expect(saved).not.toContain("secret-value");
      expect(saved).not.toContain("password");
    });

    view.unmount();
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    await waitFor(() => expect(screen.getByText(/pipeline-1: JSON Formatter/)).toBeTruthy());
    expect(screen.getByRole("button", { name: "현재 도구 즐겨찾기 해제" })).toBeTruthy();
  });

  it("opens an existing tool only after the user selects that action", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: "https://example.test/docs" } });
    fireEvent.click(screen.getByRole("button", { name: "도구 열기" }));

    expect(openTool).toHaveBeenCalledWith("url-decode");
  });

  it("preserves a corrupt metadata store and disables misleading save actions", async () => {
    const corrupt = '{"schemaVersion":1,"input":"credential-value"}';
    localStorage.setItem(WORKFLOW_STORAGE_KEY, corrupt);
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("metadata"));
    expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(input(), { target: { value: '{"safe":true}' } });
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    expect((screen.getByRole("button", { name: "파이프라인 저장" }) as HTMLButtonElement).disabled).toBe(true);
    expect(localStorage.getItem(WORKFLOW_STORAGE_KEY)).toBe(corrupt);
  });

  it("starts with a compatible next stage and describes repeated candidate actions", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);

    expect((screen.getByRole("button", { name: "단계 추가" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.change(input(), { target: { value: "deadbeef" } });

    const candidateActions = screen.getAllByRole("button", { name: "추천 단계로 사용" });
    expect(candidateActions.length).toBeGreaterThan(1);
    for (const action of candidateActions) {
      expect(action.getAttribute("aria-describedby")).toBeTruthy();
    }
  });
});
