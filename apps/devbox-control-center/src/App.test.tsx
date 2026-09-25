import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ProductShell } from "@devbox/product-shell";
import catalog from "../../products.json";
import Content from "./Content";

afterEach(() => { cleanup(); window.history.replaceState(null, "", "/"); });
const routes = catalog.features.filter((feature) => feature.owner === "control-center").map((feature) => feature.route);

it.each(routes)("renders a real view for the %s route", async (route) => {
  window.history.replaceState(null, "", `/?route=${route}`);
  const { container } = render(<ProductShell product="control-center" renderContent={Content} />);
  await screen.findByRole("navigation", { name: "제품 화면" });
  await waitFor(() => expect(screen.queryByText(/불러오고 있습니다/)).toBeNull());
  expect(screen.queryByText("기능 이전을 준비하고 있습니다")).toBeNull();
  expect(screen.queryByText("이 화면을 찾을 수 없습니다.")).toBeNull();
  if (route === "products") await assertNoA11yViolations(container);
});

it("shows global shortcut settings on the shortcuts route", async () => {
  window.history.replaceState(null, "", "/?route=shortcuts");
  render(<ProductShell product="control-center" renderContent={Content} />);
  expect(await screen.findByText("전역 단축키")).toBeTruthy();
});
