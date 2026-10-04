import * as confirmation from "@devbox/product-shell/confirm";
import { previewDocumentKey, seedPreviewDocument } from "../../storage/testDocuments";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const WORKFLOW_STORAGE_KEY = previewDocumentKey("workflows");
import { SmartWorkflowPanel } from "./SmartWorkflowPanel";

vi.mock("../api", () => ({
  readClipboardText: vi.fn(),
}));

const hosted = vi.hoisted(() => ({ value: false }));
vi.mock("../../transport", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../transport")>()),
  isProductHosted: () => hosted.value,
}));
const saveControl = vi.hoisted(() => ({ pending: null as Promise<void> | null }));
vi.mock("./workflowStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./workflowStore")>();
  return {
    ...actual,
    createWorkflowPersistence: (...args: Parameters<typeof actual.createWorkflowPersistence>) => {
      const persistence = actual.createWorkflowPersistence(...args);
      return {
        ...persistence,
        save: async (metadata: Parameters<typeof persistence.save>[0]) => {
          if (saveControl.pending) await saveControl.pending;
          await persistence.save(metadata);
        },
      };
    },
  };
});
const openTool = vi.fn();

beforeEach(() => {
  saveControl.pending = null;
  hosted.value = false;
  localStorage.removeItem(WORKFLOW_STORAGE_KEY);
  localStorage.removeItem(`${WORKFLOW_STORAGE_KEY}.revision`);
  openTool.mockReset();
});

afterEach(() => {
  cleanup();
  localStorage.removeItem(WORKFLOW_STORAGE_KEY);
  localStorage.removeItem(`${WORKFLOW_STORAGE_KEY}.revision`);
});

function input(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "스마트 워크플로 입력" }) as HTMLTextAreaElement;
}

describe("SmartWorkflowPanel", () => {
  it("shows a detection candidate and runs its selected typed stage explicitly", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: '{"name":"Ada"}' } });

    expect(screen.getByText("JSON 포매터")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    expect(screen.getByText("현재 출력 형식: JSON")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "파이프라인 실행" }));

    expect(screen.getByLabelText("파이프라인 결과").textContent).toContain('"name": "Ada"');
    expect(screen.getByText(/입력·출력은 저장하지 않으며/)).toBeTruthy();
  });

  it("does not auto-select an ambiguous Base64 representation", () => {
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: "Zm9v" } });

    expect(screen.getByText("여러 형식이 가능하므로 추천을 자동 선택하지 않았습니다.")).toBeTruthy();
    expect(screen.getByText("Base64 디코더")).toBeTruthy();
    expect(screen.getByText("Base64URL 디코더")).toBeTruthy();
    expect((screen.getByRole("button", { name: "파이프라인 실행" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("persists a restartable pipeline and favorite metadata without the draft text", async () => {
    const view = render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
    fireEvent.change(input(), { target: { value: '{"password":"secret-value"}' } });
    fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "파이프라인 저장" }) as HTMLButtonElement).disabled).toBe(false),
    );
    fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(false),
    );
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
    await waitFor(() => expect(screen.getByText(/pipeline-1: JSON 포매터/)).toBeTruthy());
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
    seedPreviewDocument("workflows", corrupt);
    render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("메타데이터"));
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

it("edits a loaded pipeline at capacity and recovers room by confirmed deletion", async () => {
  const pipelines = Array.from({ length: 20 }, (_, index) => ({
    id: `pipeline-${index + 1}`,
    inputType: "json",
    steps: [{ transformerId: "json-format" }],
    updatedAt: index + 1,
  }));
  seedPreviewDocument("workflows", JSON.stringify({ schemaVersion: 1, recentTools: [], favoriteTools: [], pipelines }));
  const confirm = vi.spyOn(confirmation, "confirmAction").mockResolvedValue(false);
  const view = render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
  fireEvent.click(await screen.findByRole("button", { name: /^pipeline-7:/ }));
  fireEvent.click(screen.getByRole("button", { name: "단계 추가" }));
  fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
  await waitFor(() => {
    const saved = JSON.parse(localStorage.getItem(WORKFLOW_STORAGE_KEY)!);
    expect(saved.pipelines).toHaveLength(20);
    expect(saved.pipelines.find((item: { id: string }) => item.id === "pipeline-7").steps).toHaveLength(2);
    expect(
      saved.pipelines
        .filter((item: { id: string }) => item.id !== "pipeline-7")
        .sort((left: { updatedAt: number }, right: { updatedAt: number }) => left.updatedAt - right.updatedAt),
    ).toEqual(pipelines.filter((item) => item.id !== "pipeline-7"));
  });
  view.unmount();
  render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
  fireEvent.click(await screen.findByRole("button", { name: /^pipeline-7:/ }));
  expect(screen.getAllByRole("button", { name: /단계 .* 제거/ })).toHaveLength(2);
  const remove = screen.getByRole("button", { name: "pipeline-7 파이프라인 삭제" });
  fireEvent.click(remove);
  expect(screen.getByRole("button", { name: /^pipeline-7:/ })).toBeTruthy();
  confirm.mockResolvedValue(true);
  fireEvent.click(remove);
  await waitFor(() => expect(JSON.parse(localStorage.getItem(WORKFLOW_STORAGE_KEY)!).pipelines).toHaveLength(19));
  fireEvent.click(screen.getByRole("button", { name: "새 파이프라인" }));
  fireEvent.change(screen.getByLabelText("파이프라인 입력 형식"), { target: { value: "json" } });
  fireEvent.click(screen.getByRole("button", { name: "단계 추가" }));
  fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
  await waitFor(() => expect(JSON.parse(localStorage.getItem(WORKFLOW_STORAGE_KEY)!).pipelines).toHaveLength(20));
  confirm.mockRestore();
});
it("completes a pipeline save when the active tool changes during persistence", async () => {
  const view = render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.change(input(), { target: { value: '{"synthetic":"private-input"}' } });
  fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
  let finish!: () => void;
  saveControl.pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
  expect(await screen.findByText("저장 중…")).toBeTruthy();
  view.rerender(<SmartWorkflowPanel activeToolId="url-decode" onOpenTool={openTool} />);
  finish();
  await screen.findByText("저장 완료");
  expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(false);
  await waitFor(() => {
    const saved = JSON.parse(localStorage.getItem(WORKFLOW_STORAGE_KEY) ?? "{}");
    expect(saved.pipelines).toEqual([expect.objectContaining({ id: "pipeline-1" })]);
    expect(saved.recentTools).toEqual(expect.arrayContaining([expect.objectContaining({ toolId: "url-decode" })]));
  });
  expect(screen.getByRole("button", { name: /pipeline-1: JSON 포매터/ })).toBeTruthy();
  expect(input().value).toContain("private-input");
});

it("reports pending and failed saves while retaining the workflow draft", async () => {
  const view = render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "현재 도구 즐겨찾기" }) as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.change(input(), { target: { value: '{"synthetic":"private-input"}' } });
  fireEvent.click(screen.getByRole("button", { name: "추천 단계로 사용" }));
  let fail!: (reason: Error) => void;
  saveControl.pending = new Promise((_resolve, reject) => {
    fail = reject;
  });
  fireEvent.click(screen.getByRole("button", { name: "파이프라인 저장" }));
  expect(await screen.findByText("저장 중…")).toBeTruthy();
  view.rerender(<SmartWorkflowPanel activeToolId="url-decode" onOpenTool={openTool} />);
  fail(new Error("synthetic failure"));
  await screen.findByRole("alert");
  expect(screen.queryByText("저장 완료")).toBeNull();
  expect(screen.queryByRole("button", { name: /pipeline-1: JSON 포매터/ })).toBeNull();
  expect(JSON.parse(localStorage.getItem(WORKFLOW_STORAGE_KEY) ?? "{}").pipelines).toEqual([]);
  expect(input().value).toContain("private-input");
  expect((screen.getByRole("button", { name: "파이프라인 저장" }) as HTMLButtonElement).disabled).toBe(true);
});

it("does not offer a handoff after a failed pipeline execution", () => {
  hosted.value = true;
  render(<SmartWorkflowPanel activeToolId="json-format" onOpenTool={openTool} />);
  fireEvent.change(screen.getByLabelText("파이프라인 입력 형식"), { target: { value: "json" } });
  fireEvent.click(screen.getByRole("button", { name: "단계 추가" }));
  fireEvent.change(input(), { target: { value: "invalid JSON" } });
  fireEvent.click(screen.getByRole("button", { name: "파이프라인 실행" }));
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "API Playground로 보내기" })).toBeNull();
});
