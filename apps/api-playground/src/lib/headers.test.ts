import { describe, expect, it } from "vitest";
import {
  addHeader,
  availableSecretNames,
  duplicateHeader,
  duplicateHeaderNameCount,
  isHeaderEnabled,
  MAX_REQUEST_HEADER_ROWS,
  normalizeHeaders,
  removeHeader,
  secretReference,
  updateHeader,
} from "./headers";

describe("request header operations", () => {
  it("legacy enabled 누락은 true로 정규화하고 false는 보존한다", () => {
    const normalized = normalizeHeaders([
      { key: "X-Trace", value: "one" },
      { key: "X-Skip", value: "two", enabled: false },
    ]);

    expect(normalized).toEqual([
      { key: "X-Trace", value: "one", enabled: true },
      { key: "X-Skip", value: "two", enabled: false },
    ]);
    expect(isHeaderEnabled(normalized[0]!)).toBe(true);
    expect(isHeaderEnabled(normalized[1]!)).toBe(false);
  });

  it("중복 header의 순서와 enabled 상태를 update/duplicate/remove에서 보존한다", () => {
    const source = [
      { key: "X-Trace", value: "one", enabled: true },
      { key: "x-trace", value: "two", enabled: false },
    ];
    const updated = updateHeader(source, 1, { enabled: true });
    const duplicated = duplicateHeader(updated, 0);
    const removed = removeHeader(duplicated, 1);

    expect(updated.map((header) => header.value)).toEqual(["one", "two"]);
    expect(updated.every(isHeaderEnabled)).toBe(true);
    expect(duplicated.map((header) => header.value)).toEqual(["one", "one", "two"]);
    expect(removed.map((header) => header.value)).toEqual(["one", "two"]);
    expect(duplicateHeaderNameCount(source)).toBe(1);
  });

  it("secret 이름만 deterministic 목록과 reference로 만들고 값은 받지 않는다", () => {
    expect(availableSecretNames(["TOKEN", "bad name", "API_KEY", "TOKEN"]))
      .toEqual(["API_KEY", "TOKEN"]);
    expect(secretReference("TOKEN")).toBe("${TOKEN}");
    expect(secretReference("bad name")).toBeNull();
  });

  it("새 header를 enabled로 추가하고 100행 상한에서 더 늘리지 않는다", () => {
    expect(addHeader([])).toEqual([{ key: "", value: "", enabled: true }]);
    const full = Array.from({ length: MAX_REQUEST_HEADER_ROWS }, (_, index) => ({
      key: `X-${index}`,
      value: String(index),
      enabled: true,
    }));
    expect(addHeader(full)).toHaveLength(MAX_REQUEST_HEADER_ROWS);
    expect(duplicateHeader(full, 0)).toHaveLength(MAX_REQUEST_HEADER_ROWS);
  });
});
