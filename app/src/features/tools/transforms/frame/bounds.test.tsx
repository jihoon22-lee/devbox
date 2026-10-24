import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { TransformerTool } from "./TransformerTool";

it("native input changes cannot bypass the automatic transform byte limit", async () => {
  const run = vi.fn(async () => ({ output: "ok" }));
  render(<TransformerTool placeholder="text" run={run} />);
  await waitFor(() => expect(screen.getByLabelText("출력")).toHaveTextContent("ok"));
  run.mockClear();
  fireEvent.change(screen.getByRole("textbox", { name: "입력" }), { target: { value: "가".repeat(350000) } });
  expect(run).not.toHaveBeenCalled();
  expect(await screen.findByText(/1MiB/)).toBeInTheDocument();
});
