import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createKnowledgeDraftHandoff,
  KNOWLEDGE_DRAFT_BROWSER_ERROR,
  KNOWLEDGE_DRAFT_CREATE_ERROR,
  KNOWLEDGE_DRAFT_INPUT_ERROR,
  KNOWLEDGE_DRAFT_INVALID_ERROR,
} from "./api";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("./lib/isTauri", () => ({ isTauri: mocks.isTauri }));

const handoffId = "0123456789abcdef0123456789abcdef";

beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.isTauri.mockReset().mockReturnValue(false);
});

afterEach(() => vi.clearAllMocks());

describe("Developer Toolbox Knowledge draft publisher API", () => {
  it("does not invoke native publishing in browser preview mode", async () => {
    await expect(createKnowledgeDraftHandoff("safe output")).rejects.toThrow(KNOWLEDGE_DRAFT_BROWSER_ERROR);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("uses the exact command and validates the typed dispatch response", async () => {
    mocks.isTauri.mockReturnValue(true);
    mocks.invoke.mockResolvedValueOnce({ handoffId, redacted: true });

    await expect(createKnowledgeDraftHandoff("safe output")).resolves.toEqual({
      handoffId,
      redacted: true,
    });
    expect(mocks.invoke).toHaveBeenCalledWith("create_knowledge_draft_handoff", {
      output: "safe output",
    });
  });

  it("rejects malformed output or native responses without invoking unsafe values", async () => {
    mocks.isTauri.mockReturnValue(true);
    await expect(createKnowledgeDraftHandoff("unsafe\0output")).rejects.toThrow(KNOWLEDGE_DRAFT_INPUT_ERROR);
    expect(mocks.invoke).not.toHaveBeenCalled();

    mocks.invoke.mockResolvedValueOnce({ handoffId: "../private", redacted: "yes" });
    await expect(createKnowledgeDraftHandoff("safe output")).rejects.toThrow(KNOWLEDGE_DRAFT_INVALID_ERROR);
  });

  it("maps unexpected native errors to a fixed message", async () => {
    mocks.isTauri.mockReturnValue(true);
    mocks.invoke.mockRejectedValueOnce(new Error("/private/output/path"));

    const rejection = createKnowledgeDraftHandoff("safe output");
    await expect(rejection).rejects.toThrow(KNOWLEDGE_DRAFT_CREATE_ERROR);
    await rejection.catch((error: unknown) => {
      expect(error instanceof Error ? error.message : String(error)).not.toContain("/private/output/path");
    });
  });
});
