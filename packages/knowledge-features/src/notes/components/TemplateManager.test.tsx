import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TemplateManager from "./TemplateManager";
import {
  createTemplate,
  deleteTemplate,
  listTemplates,
  createNoteFromTemplate,
  updateTemplate,
  type NoteTemplate,
} from "../api";

vi.mock("../api", () => ({
  createTemplate: vi.fn(),
  deleteTemplate: vi.fn(),
  listTemplates: vi.fn(),
  createNoteFromTemplate: vi.fn(),
  updateTemplate: vi.fn(),
}));

const listMock = vi.mocked(listTemplates);
const createMock = vi.mocked(createTemplate);
const deleteMock = vi.mocked(deleteTemplate);
const saveMock = vi.mocked(createNoteFromTemplate);
const updateMock = vi.mocked(updateTemplate);

const template: NoteTemplate = {
  id: 1,
  name: "Daily",
  content: "# {{title}}\n\n{{date}} {{time}}",
  createdAtMs: 1,
  updatedAtMs: 1,
};

beforeEach(() => {
  vi.resetAllMocks();
  listMock.mockResolvedValue([template]);
  createMock.mockResolvedValue(template);
  updateMock.mockResolvedValue(template);
  deleteMock.mockResolvedValue(undefined);
  saveMock.mockResolvedValue({ path: "Notes/today.md", revision: "r1" });
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

async function readyManager() {
  const dialog = await screen.findByRole("dialog", { name: "노트 템플릿" });
  // The dialog mounts before its asynchronous template selection is loaded.
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "노트 만들기" })).toBeEnabled());
  return dialog;
}

describe("Knowledge template manager", () => {
  it("creates from the saved definition with one action", async () => {
    const { onSaved } = renderManager();
    const dialog = await readyManager();
    fireEvent.click(within(dialog).getByRole("button", { name: "노트 만들기" }));
    await waitFor(() =>
      expect(saveMock).toHaveBeenCalledWith(expect.objectContaining({ templateId: 1, target: "Notes/new-note.md" })),
    );
    expect(onSaved).toHaveBeenCalledWith({ path: "Notes/today.md", revision: "r1" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
  });
  it("retains the form on failure and does not close during creation", async () => {
    let reject!: (error: Error) => void;
    saveMock.mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    );
    const { onClose, onSaved } = renderManager();
    const dialog = await readyManager();
    fireEvent.click(within(dialog).getByRole("button", { name: "노트 만들기" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    reject(new Error("기존 파일이 있습니다."));
    expect(await screen.findByRole("alert")).toHaveTextContent("기존 파일이 있습니다.");
    expect(onSaved).not.toHaveBeenCalled();
    expect(screen.getByLabelText("대상 경로")).toHaveValue("Notes/new-note.md");
  });
  it("requires saving changed template definitions before creating notes", async () => {
    renderManager();
    await readyManager();
    fireEvent.change(screen.getByLabelText("이름"), { target: { value: "Changed" } });
    expect(screen.getByRole("button", { name: "노트 만들기" })).toBeDisabled();
    expect(saveMock).not.toHaveBeenCalled();
  });
});
