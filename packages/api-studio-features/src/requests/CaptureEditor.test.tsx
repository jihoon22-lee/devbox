import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { CaptureEditor } from "./CaptureEditor";
afterEach(cleanup);
it("labels capture controls and explains invalid variable names", async () => {
  const { container } = render(
    <CaptureEditor
      value={[{ id: "c", enabled: true, variable: "bad name", source: "status", target: "" }]}
      onChange={vi.fn()}
    />,
  );
  expect(screen.getByText("변수 이름이 올바르지 않습니다")).toBeTruthy();
  await assertNoA11yViolations(container);
});
