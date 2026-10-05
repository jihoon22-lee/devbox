import { describe, expect, it } from "vitest";
import { friendlyErrorMessage } from "./api";

describe("Run Manager display error boundary", () => {
  it("maps known codes and suppresses unknown native details", () => {
    expect(friendlyErrorMessage("workspace-task-source-changed")).toBe(
      "작업 원본이 바뀌었거나 프로젝트 정의 신뢰가 철회되었습니다. 프로젝트 개요의 신뢰와 작업 실행 승인을 다시 확인하세요.",
    );
    expect(friendlyErrorMessage(new Error("native path /private/run.log"))).toBe("요청을 완료하지 못했습니다.");
  });
});

it.each(["__proto__", "constructor", "toString"])("suppresses inherited error code %s", (code) => {
  expect(friendlyErrorMessage(code)).toBe("요청을 완료하지 못했습니다.");
});
