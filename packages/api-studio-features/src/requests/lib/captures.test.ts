import { describe, expect, it } from "vitest";
import type { ApiResponse } from "../types";
import { applyCaptures } from "./captures";

const res = {
  status: 200,
  headers: [{ key: "Location", value: "/users/7" }],
  body: '{"access_token":"abc","user":{"id":7}}',
  is_json: true,
} as ApiResponse;

describe("captures", () => {
  it("extracts JSON, header and status values as strings", () => {
    const out = applyCaptures(
      [
        { id: "1", enabled: true, variable: "token", source: "jsonPath", target: "$.access_token" },
        { id: "2", enabled: true, variable: "userId", source: "jsonPath", target: "$.user.id" },
        { id: "3", enabled: true, variable: "location", source: "header", target: "location" },
        { id: "4", enabled: true, variable: "lastStatus", source: "status", target: "" },
      ],
      res,
    );
    expect(Object.fromEntries(out.values)).toEqual({
      token: "abc",
      userId: "7",
      location: "/users/7",
      lastStatus: "200",
    });
    expect(out.missing).toEqual([]);
  });
  it("reports missing values and invalid variable names", () => {
    const out = applyCaptures(
      [
        { id: "1", enabled: true, variable: "refresh", source: "jsonPath", target: "$.refresh_token" },
        { id: "2", enabled: true, variable: "bad name", source: "status", target: "" },
      ],
      res,
    );
    expect(out.missing).toEqual(["refresh"]);
    expect(out.errors).toEqual(["변수 이름이 올바르지 않습니다: bad name"]);
  });
});
it("bounds UTF-8 capture size and never retains a prior duplicate after a missing capture", () => {
  const capture = { id: "c", enabled: true, variable: "token", source: "jsonPath" as const, target: "$.access_token" };
  const oversized = applyCaptures([capture], { ...res, body: JSON.stringify({ access_token: "가".repeat(22000) }) });
  expect(oversized.values.size).toBe(0);
  expect(oversized.missing).toEqual(["token"]);
  expect(oversized.errors[0]).toContain("64KiB");
  const duplicated = applyCaptures([capture, { ...capture, id: "next", target: "$.missing" }], res);
  expect(duplicated.values.size).toBe(0);
  expect(duplicated.missing).toEqual(["token"]);
});
