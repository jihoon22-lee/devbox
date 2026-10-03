import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { SessionVariablesPanel } from "./SessionVariablesPanel";
import { SessionVariables } from "./lib/runner";
import { useLayoutEffect } from "react";
afterEach(cleanup);
it("masks captured values, reveals explicitly and clears them", async () => {
  const session = new SessionVariables();
  session.set("token", "private-token", "sealed");
  const { container } = render(<SessionVariablesPanel session={session} onChange={vi.fn()} />);
  expect(screen.queryByText("private-token")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "token 보기" }));
  expect(await screen.findByText("private-token")).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "token 지우기" }));
  expect(session.entries()).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "되돌리기" }));
  expect(session.entries()).toEqual([{ name: "token", plain: "private-token" }]);
});

it("reveals native captures only on demand and ignores a reply after deleting the variable", async () => {
  let finish!: (value: string) => void;
  const access = {
    reveal: vi.fn(
      () =>
        new Promise<string>((resolve) => {
          finish = resolve;
        }),
    ),
    discard: vi.fn(async () => {}),
    restore: vi.fn(async () => {}),
  };
  const session = new SessionVariables(access);
  session.setNative({ name: "token", value: "sealed", reference: "ref" });
  const { container } = render(<SessionVariablesPanel session={session} onChange={vi.fn()} />);
  expect(access.reveal).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "token 보기" }));
  expect(access.reveal).toHaveBeenCalledWith("ref");
  fireEvent.click(screen.getByRole("button", { name: "token 지우기" }));
  finish("late-private-token");
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(screen.queryByText("late-private-token")).toBeNull();
  await assertNoA11yViolations(container);
});

it("clears a revealed value when a new capture replaces it", async () => {
  const session = new SessionVariables({
    reveal: vi.fn(async () => "native-private-token"),
    discard: vi.fn(async () => {}),
    restore: vi.fn(async () => {}),
  });
  session.setNative({ name: "token", value: "sealed", reference: "ref" });
  const { rerender, container } = render(<SessionVariablesPanel session={session} onChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "token 보기" }));
  expect(await screen.findByText("native-private-token")).toBeTruthy();
  session.setNative({ name: "token", value: "next-sealed", reference: "next-ref" });
  rerender(<SessionVariablesPanel session={session} onChange={vi.fn()} />);
  expect(screen.queryByText("native-private-token")).toBeNull();
  await assertNoA11yViolations(container);
});

it("keeps a reveal started as soon as a new capture is committed", async () => {
  const session = new SessionVariables({
    reveal: vi.fn(async () => "just-captured-token"),
    discard: vi.fn(async () => {}),
    restore: vi.fn(async () => {}),
  });
  function Host({ revealOnCommit }: { revealOnCommit: boolean }) {
    useLayoutEffect(() => {
      if (revealOnCommit) screen.getByRole("button", { name: "token 보기" }).click();
    }, [revealOnCommit]);
    return <SessionVariablesPanel session={session} onChange={vi.fn()} />;
  }
  const { rerender } = render(<Host revealOnCommit={false} />);
  session.setNative({ name: "token", value: "sealed", reference: "ref" });
  rerender(<Host revealOnCommit />);
  expect(await screen.findByText("just-captured-token")).toBeTruthy();
});
