import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TemplateManager from "./TemplateManager";
import {
  createTemplate,
  deleteTemplate,
  discardTemplatePreview,
  listTemplates,
  previewTemplate,
  saveTemplate,
  updateTemplate,
  type NoteTemplate,
  type TemplatePreview,
} from "../api";

vi.mock("../api", () => ({
  createTemplate: vi.fn(),
  deleteTemplate: vi.fn(),
  discardTemplatePreview: vi.fn(),
  listTemplates: vi.fn(),
  previewTemplate: vi.fn(),
  saveTemplate: vi.fn(),
  updateTemplate: vi.fn(),
}));

const listMock = vi.mocked(listTemplates);
const createMock = vi.mocked(createTemplate);
const deleteMock = vi.mocked(deleteTemplate);
const discardMock = vi.mocked(discardTemplatePreview);
const previewMock = vi.mocked(previewTemplate);
const saveMock = vi.mocked(saveTemplate);
const updateMock = vi.mocked(updateTemplate);

const template: NoteTemplate = {
  id: 1,
  name: "Daily",
  content: "# {{title}}\n\n{{date}} {{time}}",
  createdAtMs: 1,
  updatedAtMs: 1,
};

const preview: TemplatePreview = {
  previewId: "tpl-1",
  templateId: 1,
  templateUpdatedAtMs: 1,
  target: "Notes/today.md",
  content: "# Today\n\n2026-08-28 09:00",
  byteLength: 30,
};

beforeEach(() => {
  listMock.mockResolvedValue([template]);
  createMock.mockResolvedValue(template);
  updateMock.mockResolvedValue(template);
  deleteMock.mockResolvedValue(undefined);
  discardMock.mockResolvedValue(undefined);
  previewMock.mockResolvedValue(preview);
  saveMock.mockResolvedValue({ saved: true, path: preview.target });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderManager(onClose = vi.fn(), onSaved = vi.fn()) {
  return {
    ...render(<TemplateManager onClose={onClose} onSaved={onSaved} />),
    onClose,
    onSaved,
  };
}

describe("Knowledge template manager", () => {
  it("exposes labelled nested dialogs and consumes a confirmed preview", async () => {
    const { onSaved } = renderManager();
    const dialog = await screen.findByRole("dialog", { name: "노트 템플릿" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("aria-describedby", "template-manager-description");
    expect(within(dialog).getByLabelText("이름")).toHaveValue("Daily");

    fireEvent.click(within(dialog).getByRole("button", { name: "적용 전 미리보기" }));
    const previewDialog = await screen.findByRole("dialog", { name: /미리보기 · Notes\/today\.md/u });
    expect(previewDialog).toHaveAttribute("aria-describedby", "template-preview-description");
    expect(saveMock).not.toHaveBeenCalled();

    fireEvent.click(within(previewDialog).getByRole("button", { name: "노트 만들기" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledWith("tpl-1"));
    expect(onSaved).toHaveBeenCalledWith({ saved: true, path: "Notes/today.md" });
  });

  it("does not leave a stale native preview after an unmounted request resolves", async () => {
    let resolvePreview: ((value: TemplatePreview) => void) | undefined;
    previewMock.mockImplementationOnce(() => new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    const { unmount } = renderManager();
    const dialog = await screen.findByRole("dialog", { name: "노트 템플릿" });
    fireEvent.click(within(dialog).getByRole("button", { name: "적용 전 미리보기" }));
    await waitFor(() => expect(previewMock).toHaveBeenCalledTimes(1));
    unmount();

    resolvePreview?.(preview);
    await waitFor(() => expect(discardMock).toHaveBeenCalledWith("tpl-1"));
  });

  it("keeps the approval visible when cancellation fails", async () => {
    discardMock.mockRejectedValueOnce(new Error("temporary discard failure"));
    renderManager();
    const dialog = await screen.findByRole("dialog", { name: "노트 템플릿" });
    fireEvent.click(within(dialog).getByRole("button", { name: "적용 전 미리보기" }));
    const previewDialog = await screen.findByRole("dialog", { name: /미리보기 · Notes\/today\.md/u });

    fireEvent.click(within(previewDialog).getByRole("button", { name: "취소" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("temporary discard failure");
    expect(screen.getByRole("dialog", { name: /미리보기 · Notes\/today\.md/u })).toBeInTheDocument();
    expect(saveMock).not.toHaveBeenCalled();
  });
});
