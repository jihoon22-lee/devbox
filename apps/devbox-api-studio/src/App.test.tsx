import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ProductShell } from "@devbox/product-shell";
afterEach(cleanup);
it("opens the api-studio shell with accessible native-mock status", async () => {
  const { container } = render(<ProductShell product="api-studio" />);
  await screen.findByText("기능 이전을 준비하고 있습니다");
  expect(screen.getByRole("navigation", { name: "제품 화면" })).toBeTruthy();
  await assertNoA11yViolations(container);
});
