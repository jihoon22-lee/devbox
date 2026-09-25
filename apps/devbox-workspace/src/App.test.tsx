import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ProductShell } from "@devbox/product-shell";
afterEach(cleanup);
it("opens the workspace shell with accessible native-mock status", async () => {
  const { container } = render(<ProductShell product="workspace" />);
  await screen.findByText("이 화면은 아직 제공되지 않습니다");
  expect(screen.getByRole("navigation", { name: "제품 화면" })).toBeTruthy();
  await assertNoA11yViolations(container);
});
