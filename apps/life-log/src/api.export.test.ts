import { describe, expect, it } from "vitest";
import { buildExportInput } from "./App";
import { exportLifeLog } from "./api";

function fixture(format: "markdown" | "json" | "csv") {
  const input = buildExportInput("2024-01-01", "2024-01-02", format);
  if (!input) throw new Error("fixture range could not be built");
  return input;
}

describe("Life Log browser export preview", () => {
  it("keeps native data unavailable and produces byte-identical JSON", async () => {
    const input = fixture("json");
    const first = await exportLifeLog(input);
    const second = await exportLifeLog(input);

    expect(first).toEqual(second);
    expect(first.origin).toBe("browser-preview");
    expect(first.extension).toBe("json");
    expect(first.mimeType).toBe("application/json;charset=utf-8");
    expect(first.byteLength).toBe(new TextEncoder().encode(first.content).byteLength);

    const document = JSON.parse(first.content) as {
      summary: { sessionCount: number };
      sources: Array<{ id: string; available: boolean; errorCode: string; scope: string }>;
    };
    expect(document.summary.sessionCount).toBe(0);
    expect(document.sources).toHaveLength(4);
    expect(document.sources.map((source) => source.id)).toEqual([
      "life-log",
      "git",
      "run-manager",
      "knowledge-base",
    ]);
    expect(document.sources.every((source) =>
      !source.available
      && source.errorCode === "browser_preview_only"
      && source.scope === "browser-preview-only",
    )).toBe(true);
    expect(first.content).not.toContain("C:\\secret");
  });

  it("uses the fixed 24-column CRLF CSV preview contract", async () => {
    const result = await exportLifeLog(fixture("csv"));
    expect(result.origin).toBe("browser-preview");
    expect(result.extension).toBe("csv");
    expect(result.mimeType).toBe("text/csv;charset=utf-8");
    expect(result.byteLength).toBe(new TextEncoder().encode(result.content).byteLength);
    const records = result.content.split("\r\n");
    expect(records[records.length - 1]).toBe("");
    expect(records[0]?.split(",")).toHaveLength(24);
    expect(records.slice(1, -1)).toHaveLength(4);
    for (const record of records.slice(1, -1)) {
      expect(record.split(",")).toHaveLength(24);
    }
    expect(result.content).toContain("browser_preview_only");
    expect(result.content.split("\r\n").join("")).not.toContain("\n");
  });

  it("escapes browser Markdown table cells", async () => {
    const input = { ...fixture("markdown"), timezone: "Zone|name\\`tick" };
    const result = await exportLifeLog(input);

    expect(result.content).toContain("- Timezone: Zone\\|name\\\\\\`tick");
  });

  it("rejects malformed preview boundaries without echoing raw input", async () => {
    const input = fixture("markdown");
    const raw = "C:\\secret\\credential\u0000value";
    const malformed = { ...input, timezone: raw };

    await expect(exportLifeLog(malformed)).rejects.toThrow("브라우저 미리보기 입력이 올바르지 않습니다");
    await expect(exportLifeLog(malformed)).rejects.not.toThrow(raw);

    await expect(
      exportLifeLog({ ...input, credential: raw } as typeof input),
    ).rejects.toThrow("브라우저 미리보기 입력이 올바르지 않습니다");
  });
});
