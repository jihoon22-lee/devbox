import { describe, expect, it } from "vitest";
import operations from "../fixtures/operations.json";
import { isOperation, operationMessage, problemMessage } from "./operation";

describe("native operation observations", () => {
  const expected = operations[0].provenance;
  it("renders shared cancellation, stale and failure fixtures distinctly", () => {
    const messages = operations.map((value) => {
      expect(isOperation(value, expected)).toBe(true);
      if (!isOperation(value, expected)) throw new Error("invalid fixture");
      return operationMessage(value);
    });
    expect(new Set(messages).size).toBe(operations.length);
  });
  it("rejects stale revisions, foreign owners and future outcome fields", () => {
    for (const mutation of [{ product: "knowledge" }, { component: "workspace.runtime" }, { requestId: "old" }, { revision: 2 }]) {
      expect(isOperation({ ...operations[0], provenance: { ...expected, ...mutation } }, expected)).toBe(false);
    }
    for (const outcome of [{ state: "failed" }, { state: "failed", code: "future" }, { state: "succeeded", code: "expired" }, { state: "running", path: "private" }]) {
      expect(isOperation({ provenance: expected, outcome }, expected)).toBe(false);
    }
  });
  it("never displays native error payloads or unverified problem codes", () => {
    expect(problemMessage({ code: "expired", provenance: expected }, expected)).toBe("작업 시간이 초과되었습니다.");
    for (const error of ["secret native path", { code: "secret" }, { code: "expired", provenance: { ...expected, requestId: "old" } }]) {
      expect(problemMessage(error, expected)).toBe("작업 상태를 확인할 수 없습니다.");
    }
  });
});
