import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { AssertionEditor } from "./AssertionEditor";
import type { Assertion } from "./lib/assertions";
afterEach(cleanup);
it("edits assertion fields, validates paths and hides unused fields", async () => {
  function Harness() {
    const [value, setValue] = useState<Assertion[]>([]);
    return <AssertionEditor value={value} onChange={setValue} />;
  }
  const { container } = render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "검증 추가" }));
  expect(screen.queryByLabelText("대상 1")).toBeNull();
  fireEvent.change(screen.getByLabelText("출처 1"), { target: { value: "jsonPath" } });
  fireEvent.change(screen.getByLabelText("대상 1"), { target: { value: "$[" } });
  expect(screen.getByText("지원하지 않는 JSONPath입니다")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("연산 1"), { target: { value: "exists" } });
  expect(screen.queryByLabelText("기대값 1")).toBeNull();
  await assertNoA11yViolations(container);
});
it("caps assertions at fifty", () => {
  render(
    <AssertionEditor
      value={Array.from({ length: 50 }, (_, i) => ({
        id: String(i),
        enabled: true,
        source: "status",
        target: "",
        operator: "equals",
        expected: "200",
      }))}
      onChange={vi.fn()}
    />,
  );
  expect((screen.getByRole("button", { name: "검증 추가" }) as HTMLButtonElement).disabled).toBe(true);
});
