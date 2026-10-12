import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { CommandPalette } from "./CommandPalette";
import { useCommands } from "./commands";

beforeEach(() => useCommands.setState({ items: new Map() }));
test("old registration cleanup cannot delete a replacement", () => {
  const command = { id: "one", label: "명령", run: vi.fn() };
  const off = useCommands.getState().register(command);
  useCommands.getState().register(command);
  off();
  expect(useCommands.getState().list()).toHaveLength(1);
});
test("filters Korean commands, handles empty results, restores focus and resets reopening", async () => {
  const run = vi.fn();
  useCommands.getState().register({ id: "switch", label: "프로젝트 전환", run });
  useCommands.getState().register({ id: "settings", label: "설정 열기", run: vi.fn() });
  function Test() {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button onClick={() => setOpen(true)}>열기</button>
        <CommandPalette open={open} onOpenChange={setOpen} />
      </>
    );
  }
  render(<Test />);
  const user = userEvent.setup();
  await user.click(screen.getByText("열기"));
  const input = screen.getByRole("combobox");
  await user.type(input, "없음");
  fireEvent.keyDown(input, { key: "ArrowDown" });
  await user.keyboard("{Enter}");
  expect(run).not.toHaveBeenCalled();
  await user.clear(input);
  await user.type(input, "전환");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(document.getElementById(input.getAttribute("aria-activedescendant") ?? "")).toHaveTextContent("프로젝트 전환");
  await user.keyboard("{Enter}");
  expect(run).toHaveBeenCalledOnce();
  await waitFor(() => expect(screen.getByText("열기")).toHaveFocus());
  await user.click(screen.getByText("열기"));
  expect(screen.getByRole("combobox")).toHaveValue("");
});
test("selection clamps on registry removal and arrows prevent default", () => {
  const first = vi.fn(),
    second = vi.fn();
  useCommands.getState().register({ id: "one", label: "하나", run: first });
  const off = useCommands.getState().register({ id: "two", label: "둘", run: second });
  render(<CommandPalette open onOpenChange={() => {}} />);
  const input = screen.getByRole("combobox");
  expect(fireEvent.keyDown(input, { key: "ArrowDown" })).toBe(false);
  act(off);
  fireEvent.keyDown(input, { key: "Enter" });
  expect(first).toHaveBeenCalledOnce();
  expect(second).not.toHaveBeenCalled();
});
