import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FirstRun } from "./FirstRun";

test("autostart starts checked, cancelling changes nothing and submit records the explicit choice once", async () => {
  const done = vi.fn().mockResolvedValue(undefined);
  const close = vi.fn();
  render(<FirstRun onDone={done} onCancel={close} />);
  expect(screen.getByRole("checkbox", { name: "Windows 시작 시 devbox 실행" })).toBeChecked();
  expect(done).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "나중에" })).toHaveFocus();
  await userEvent.click(screen.getByRole("checkbox"));
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  expect(done).toHaveBeenCalledWith({ autostart: false });
  expect(done).toHaveBeenCalledTimes(1);
});
test("failed settings remain retryable and duplicate pending submissions are disabled", async () => {
  let reject!: (reason: unknown) => void;
  const done = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    )
    .mockResolvedValue(undefined);
  render(<FirstRun onDone={done} onCancel={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  expect(screen.getByRole("button", { name: "시작하기" })).toBeDisabled();
  reject(new Error("owned fixture failure"));
  expect(await screen.findByRole("alert")).toHaveTextContent("설정을 저장하지 못했습니다");
  await userEvent.click(screen.getByRole("button", { name: "시작하기" }));
  await waitFor(() => expect(done).toHaveBeenCalledTimes(2));
});
test("first run describes keeping WSL connected and cancelling defers the choice", async () => {
  const close = vi.fn();
  const done = vi.fn();
  render(<FirstRun onDone={done} onCancel={close} />);
  expect(screen.getByText(/트레이에서 실행되는 동안/)).toHaveTextContent("WSL");
  expect(screen.getByText(/instanceIdleTimeout/)).toHaveTextContent("15초");
  await userEvent.click(screen.getByRole("button", { name: "나중에" }));
  expect(close).toHaveBeenCalledOnce();
  expect(done).not.toHaveBeenCalled();
});
