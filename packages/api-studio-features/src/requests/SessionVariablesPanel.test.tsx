import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { SessionVariablesPanel } from "./SessionVariablesPanel";
import { SessionVariables } from "./lib/runner";
afterEach(cleanup);
it("masks captured values, reveals explicitly and clears them", async () => {
  const session = new SessionVariables();
  session.set("token", "private-token", "sealed");
  const { container } = render(<SessionVariablesPanel session={session} onChange={vi.fn()} />);
  expect(screen.queryByText("private-token")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "token 보기" }));
  expect(screen.getByText("private-token")).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "token 지우기" }));
  expect(session.entries()).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  expect(session.entries()).toEqual([{ name: "token", plain: "private-token" }]);
});
