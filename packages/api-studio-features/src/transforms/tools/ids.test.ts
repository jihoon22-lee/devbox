import { afterEach, describe, expect, it, vi } from "vitest";
import {
  encodeCrockfordUlid,
  generateIdentifiers,
  IDENTIFIER_SEQUENCE_ERROR,
  IDENTIFIER_GENERATION_ERROR,
  MAX_IDENTIFIER_BATCH,
  SECURE_RANDOM_ERROR,
  type IdentifierKind,
} from "./ids";

const CROCKFORD = /^[0-9A-HJKMNP-TV-Z]+$/;

function options(kind: IdentifierKind, overrides: Partial<Parameters<typeof generateIdentifiers>[0]> = {}) {
  return {
    kind,
    count: 2,
    uppercase: false,
    hyphens: false,
    ...overrides,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("identifier generator", () => {
  it("generates UUID v4 in compact and hyphenated forms", () => {
    const compact = generateIdentifiers(options("uuid-v4"));
    expect(compact).toHaveLength(2);
    expect(compact.every((value) => /^[0-9a-f]{32}$/.test(value))).toBe(true);
    expect(compact.every((value) => value[12] === "4")).toBe(true);

    const formatted = generateIdentifiers(
      options("uuid-v4", { count: 1, uppercase: true, hyphens: true }),
    )[0];
    expect(formatted).toMatch(/^[0-9A-F]{8}(?:-[0-9A-F]{4}){3}-[0-9A-F]{12}$/);
    expect(formatted[14]).toBe("4");
  });

  it("generates time-ordered UUID v7 shape with RFC variant bits", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    try {
      const values = generateIdentifiers(options("uuid-v7", { count: 4, hyphens: true }));
      expect(values).toHaveLength(4);
      expect(values.every((value) => value.length === 36 && value[14] === "7")).toBe(true);
      expect(values.every((value) => /^[0-9a-f-]+$/.test(value))).toBe(true);
      expect(values.every((value) => ["8", "9", "a", "b"].includes(value[19]))).toBe(true);
      expect(values.every((value, index) => index === 0 || values[index - 1] < value)).toBe(true);
    } finally {
      now.mockRestore();
    }
  });

  it("keeps UUID v7 and ULID ordered when the wall clock moves backwards", () => {
    const uuidClock = vi
      .spyOn(Date, "now")
      .mockReturnValueOnce(1_700_000_000_000)
      .mockReturnValueOnce(1_600_000_000_000);
    const uuidValues = generateIdentifiers(options("uuid-v7", { count: 2 }));
    uuidClock.mockRestore();

    const ulidClock = vi
      .spyOn(Date, "now")
      .mockReturnValueOnce(1_700_000_000_000)
      .mockReturnValueOnce(1_600_000_000_000);
    const ulidValues = generateIdentifiers(options("ulid", { count: 2, uppercase: true }));

    expect(uuidValues[0] < uuidValues[1]).toBe(true);
    expect(ulidValues[0] < ulidValues[1]).toBe(true);
    ulidClock.mockRestore();
  });

  it("generates canonical Crockford ULIDs and display variants", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    try {
      const canonical = generateIdentifiers(options("ulid", { count: 3, uppercase: true }));
      expect(canonical).toHaveLength(3);
      expect(canonical.every((value) => value.length === 26 && CROCKFORD.test(value))).toBe(true);
      expect(canonical.every((value) => value[0] <= "7")).toBe(true);
      expect(canonical.every((value, index) => index === 0 || canonical[index - 1] < value)).toBe(true);

      const grouped = generateIdentifiers(
        options("ulid", { count: 1, uppercase: false, hyphens: true }),
      )[0];
      expect(grouped).toMatch(/^[0-9a-hjkmnp-tv-z]{5}(?:-[0-9a-hjkmnp-tv-z]{5}){3}-[0-9a-hjkmnp-tv-z]{6}$/);
      expect(grouped.split("-").join("")).toHaveLength(26);
    } finally {
      now.mockRestore();
    }
  });

  it("matches the canonical ULID boundary vectors", () => {
    expect(encodeCrockfordUlid(new Uint8Array(16))).toBe("00000000000000000000000000");
    expect(encodeCrockfordUlid(new Uint8Array(16).fill(0xff))).toBe(`7${"Z".repeat(25)}`);
  });

  it("matches the published ULID encoding vector", () => {
    expect(
      encodeCrockfordUlid(
        Uint8Array.from([
          0x01, 0x56, 0x3e, 0x3a, 0xb5, 0xd3, 0xd6, 0x76,
          0x4c, 0x61, 0xef, 0xb9, 0x93, 0x02, 0xbd, 0x5b,
        ]),
      ),
    ).toBe("01ARZ3NDEKTSV4RRFFQ69G5FAV");
  });

  it("maps browser CSPRNG failures to a fixed safe error", () => {
    vi.stubGlobal("crypto", {
      getRandomValues: () => {
        throw new Error("raw platform detail");
      },
    });
    try {
      expect(() => generateIdentifiers(options("uuid-v4", { count: 1 }))).toThrow(
        SECURE_RANDOM_ERROR,
      );
      expect(IDENTIFIER_GENERATION_ERROR).not.toContain("raw platform detail");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("uses deterministic CSPRNG bytes while preserving UUID version and variant bits", () => {
    const getRandomValues = vi.fn((bytes: Uint8Array) => {
      bytes.fill(0);
      return bytes;
    });
    vi.stubGlobal("crypto", { getRandomValues });

    expect(generateIdentifiers(options("uuid-v4", { count: 1, hyphens: true }))).toEqual([
      "00000000-0000-4000-8000-000000000000",
    ]);
    expect(getRandomValues).toHaveBeenCalledTimes(1);
  });

  it("fails closed when a monotonic suffix is exhausted at the timestamp bound", () => {
    vi.spyOn(Date, "now").mockReturnValue(0xffffffffffff);
    vi.stubGlobal("crypto", {
      getRandomValues: (bytes: Uint8Array) => {
        bytes.fill(0xff);
        return bytes;
      },
    });

    expect(() => generateIdentifiers(options("uuid-v7", { count: 2 }))).toThrow(
      IDENTIFIER_SEQUENCE_ERROR,
    );
    expect(() => generateIdentifiers(options("ulid", { count: 2, uppercase: true }))).toThrow(
      IDENTIFIER_SEQUENCE_ERROR,
    );
  });

  it("enforces a bounded non-empty batch", () => {
    expect(() => generateIdentifiers(options("uuid-v4", { count: 0 }))).toThrow();
    expect(() =>
      generateIdentifiers(options("uuid-v4", { count: MAX_IDENTIFIER_BATCH + 1 })),
    ).toThrow();
    expect(() =>
      generateIdentifiers(options("uuid-v4", { uppercase: "yes" as unknown as boolean })),
    ).toThrow();
  });
});
