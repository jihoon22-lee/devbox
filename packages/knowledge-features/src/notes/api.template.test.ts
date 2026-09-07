import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ readText: vi.fn() }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => false }));

import {
  createTemplate,
  deleteTemplate,
  previewTemplate,
  saveTemplate,
  updateTemplate,
} from "./api";

const input = {
  templateId: 1,
  target: "Notes/today.md",
  title: "Today",
  date: "2026-08-28",
  time: "09:00",
};

describe("Knowledge browser template contract", () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it("rejects controls, Windows separators, and year-zero dates before preview", async () => {
    await expect(previewTemplate({ ...input, target: "Notes/unsafe\u0001.md" }))
      .rejects.toThrow("템플릿 저장 경로가 올바르지 않습니다");
    await expect(previewTemplate({ ...input, target: "Notes\\unsafe.md" }))
      .rejects.toThrow("템플릿 저장 경로가 올바르지 않습니다");
    await expect(previewTemplate({ ...input, date: "0000-01-01" }))
      .rejects.toThrow("템플릿 날짜가 올바르지 않습니다");
    await expect(createTemplate({ name: "unsafe\u0001", content: "body" }))
      .rejects.toThrow("템플릿 이름이 올바르지 않습니다");
  });

  it("reports browser approval as preview-only and consumes it once", async () => {
    const preview = await previewTemplate(input);
    const result = await saveTemplate(preview.previewId);
    expect(result).toEqual({ saved: false, path: input.target });
    await expect(saveTemplate(preview.previewId)).rejects.toThrow("템플릿 미리보기가 없습니다");
  });

  it("keeps placeholder-looking substitution values literal", async () => {
    const template = await createTemplate({
      name: `Literal values ${Date.now()}`,
      content: "{{title}} · {{vault-relative-path}}",
    });
    const preview = await previewTemplate({
      ...input,
      templateId: template.id,
      title: "literal {{date}}",
      target: "Notes/{{title}}.md",
    });
    expect(preview.content).toBe("literal {{date}} · Notes/{{title}}.md");
    await deleteTemplate(template.id);
  });

  it("rejects a preview after its template definition revision changes", async () => {
    const preview = await previewTemplate(input);
    await updateTemplate(1, { name: "Daily revised", content: "# {{title}}" });
    await expect(saveTemplate(preview.previewId)).rejects.toThrow("템플릿 미리보기가 오래되어 다시 확인하세요");
  });

  it("matches native invalid-id behavior for browser delete", async () => {
    await expect(deleteTemplate(999_999)).rejects.toThrow("템플릿을 찾을 수 없습니다");
  });

  it("trims names and rejects case-insensitive browser duplicates like native", async () => {
    const created = await createTemplate({ name: "  Review notes  ", content: "# {{title}}" });
    expect(created.name).toBe("Review notes");
    await expect(createTemplate({ name: "review NOTES", content: "body" }))
      .rejects.toThrow("템플릿 이름이 이미 있습니다");
    await deleteTemplate(created.id);
  });
});
