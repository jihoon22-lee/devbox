import { describe, expect, it } from "vitest";
import { buildExportInput } from "./App";
import { exportLifeLog, getDigest, validateDigestInput, type DigestInput } from "./api";

function fixture(format: "markdown" | "json" | "csv") {
  const input = buildExportInput("2024-01-01", "2024-01-02", format);
  if (!input) throw new Error("fixture range could not be built");
  return input;
}

function digestInput(
  input: NonNullable<ReturnType<typeof buildExportInput>>,
  period: DigestInput["period"],
  app: string | null = null,
): DigestInput {
  return {
    startDate: input.startDate,
    endDate: input.endDate,
    timezone: input.timezone,
    dayStart: input.dayStart,
    dayEnd: input.dayEnd,
    dayBoundaries: input.dayBoundaries,
    period,
    filter: { app },
  };
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

describe("Life Log browser local digest preview", () => {
  it("keeps a daily preview to one local civil day and preserves its boundary", async () => {
    const exportInput = buildExportInput("2024-01-03", "2024-01-03", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const input = digestInput(exportInput, "day");
    const result = await getDigest(input);

    expect(result.origin).toBe("browser-preview");
    expect(result.document.period).toBe("day");
    expect(result.document.range.dayBoundaries).toEqual(input.dayBoundaries);
    expect(result.document.daily).toHaveLength(1);
    expect(result.document.summary.totalDays).toBe(1);
    expect(result.document.sources.every((source) =>
      !source.available
      && source.scope === "browser-preview-only"
      && source.errorCode === "browser_preview_only",
    )).toBe(true);
    expect(result.markdown).toContain("Period: `day`");
    expect(result.markdown).toContain("date keys inclusive; end timestamp exclusive");
  });

  it("keeps all native sources unavailable and preserves daily boundaries", async () => {
    const exportInput = buildExportInput("2024-01-01", "2024-01-07", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const input = digestInput(exportInput, "week");
    const result = await getDigest(input);
    expect(result.origin).toBe("browser-preview");
    expect(result.document.period).toBe("week");
    expect(result.document.daily).toHaveLength(7);
    expect(result.document.summary).toMatchObject({ sessionCount: 0, activeDays: 0, gitCommits: 0 });
    expect(result.document.sources.map((source) => source.id)).toEqual([
      "life-log", "git", "run-manager", "knowledge-base",
    ]);
    expect(result.document.sources.every((source) =>
      !source.available && source.errorCode === "browser_preview_only",
    )).toBe(true);
    expect(result.markdown).toContain("## Rules\n\n");
    expect(result.markdown).not.toContain("C:\\secret");
  });

  it("renders a deterministic empty monthly preview without native data", async () => {
    const exportInput = buildExportInput("2024-02-01", "2024-02-29", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const input = digestInput(exportInput, "month");
    const first = await getDigest(input);
    const second = await getDigest(input);
    expect(first).toEqual(second);
    expect(first.document.daily).toHaveLength(29);
    expect(first.document.summary.totalDays).toBe(29);
    expect(first.document.appTotals).toEqual([]);
    expect(first.markdown).toContain("Period: `month`");
    expect(first.markdown).toContain("No activity was recorded in the browser preview.");
  });

  it("rejects a monthly range whose end is not the actual calendar month end", () => {
    const exportInput = buildExportInput("2024-02-01", "2024-02-28", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    expect(() => validateDigestInput(digestInput(exportInput, "month"))).toThrow(
      "digest 입력이 올바르지 않습니다",
    );
  });

  it("rejects an app filter that resembles a credential without echoing it", () => {
    const exportInput = buildExportInput("2024-01-01", "2024-01-07", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const raw = "Authorization: bearer secret-value";
    expect(() => validateDigestInput(digestInput(exportInput, "week", raw))).toThrow("digest 입력이 올바르지 않습니다");
    try {
      validateDigestInput(digestInput(exportInput, "week", raw));
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).not.toContain(raw);
    }
  });

  it("rejects malformed digest boundaries before a browser preview is built", () => {
    const exportInput = buildExportInput("2024-01-01", "2024-01-07", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const raw = "C:\\secret\\credential\u0000value";
    const malformed = { ...digestInput(exportInput, "week"), timezone: raw };
    expect(() => validateDigestInput(malformed)).toThrow("digest 입력이 올바르지 않습니다");
    try {
      validateDigestInput(malformed);
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).not.toContain(raw);
    }
  });

  it("rejects a boundary width that is neither a normal day nor a DST day", () => {
    const exportInput = buildExportInput("2024-01-01", "2024-01-01", "json");
    if (!exportInput) throw new Error("fixture range could not be built");
    const boundary = exportInput.dayBoundaries[0];
    if (!boundary) throw new Error("fixture boundary missing");
    const malformed = digestInput({
      ...exportInput,
      dayEnd: boundary.endMs + 1,
      dayBoundaries: [{ ...boundary, endMs: boundary.endMs + 1 }],
    }, "day");
    expect(() => validateDigestInput(malformed)).toThrow("digest 입력이 올바르지 않습니다");
  });

  it("rejects a non-Monday weekly range and a multi-day daily range", () => {
    const weekRange = buildExportInput("2024-01-02", "2024-01-08", "json");
    const dayRange = buildExportInput("2024-01-03", "2024-01-04", "json");
    if (!weekRange || !dayRange) throw new Error("fixture range could not be built");

    expect(() => validateDigestInput({
      ...digestInput(weekRange, "week"),
    })).toThrow("digest 입력이 올바르지 않습니다");
    expect(() => validateDigestInput({
      ...digestInput(dayRange, "day"),
    })).toThrow("digest 입력이 올바르지 않습니다");
  });
});
