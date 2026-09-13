import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../transport", () => ({ componentInvoke: () => native.invoke }));
import { KnowledgeDraftAction } from "./KnowledgeDraftAction";
import { parseDraft } from "./api";
const owner = "api-studio.transforms" as const;
const fixture = () => ({
  artifact: { id: "01234567-89ab-4cde-8fab-0123456789ab", kind: "knowledge-draft/v1",
    provenance: { product: "api-studio", component: owner, requestId: "fixture", revision: 1 } },
  createdAtMs: 1000, title: "API Studio · Transforms 결과", body: "safe\n[REDACTED]", redacted: true,
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });
describe("producer-owned Knowledge fallback", () => {
  it("requires explicit saving and reports unavailable with a durable export", async () => {
    native.invoke.mockResolvedValue({ delivery: "stored", draft: fixture() });
    render(<KnowledgeDraftAction value={"safe\npassword=synthetic-secret"} owner={owner} source={{ kind: "tool", toolId: "json-format" }} />);
    expect(native.invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Knowledge 초안 보관" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(native.invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "마스킹 사본 보관" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("API Studio에 마스킹한 초안을 보관했습니다"));
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith("save_knowledge_draft", {
      output: "safe\npassword=synthetic-secret", source: { kind: "tool", toolId: "json-format" },
    });
    expect(screen.getByRole("button", { name: "보관한 초안 내보내기" })).toBeTruthy();
    expect(document.body.textContent).not.toContain("synthetic-secret");
    expect(document.body.textContent).not.toContain("전달했습니다");
  });
  it("rejects a foreign owner or future artifact kind before showing stored output", () => {
    const draft = fixture();
    expect(parseDraft(draft, owner).body).toBe("safe\n[REDACTED]");
    expect(() => parseDraft(draft, "api-studio.api")).toThrow();
    draft.artifact.kind = "knowledge-draft/v999";
    expect(() => parseDraft(draft, owner)).toThrow();
  });
});
