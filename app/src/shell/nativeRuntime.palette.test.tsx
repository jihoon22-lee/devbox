import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, useState } from "react";
import { Dialog } from "../ui";
import { CommandPalette } from "./CommandPalette";
import { useCommands } from "./commands";
import { consumeNativeIntents, useNativeRuntime } from "./nativeRuntime";

beforeEach(() => {
  useNativeRuntime.setState({ intents: [] });
  useCommands.setState({ items: new Map() });
});
afterEach(cleanup);
function PaletteHost({ blocked = false }: { blocked?: boolean }) {
  const [open, setOpen] = useState(!blocked);
  const [other, setOther] = useState(blocked);
  useEffect(
    () =>
      consumeNativeIntents((intent) => {
        if (intent.kind === "palette") setOpen(true);
      }),
    [],
  );
  return (
    <>
      <CommandPalette open={open} onOpenChange={setOpen} />
      <Dialog open={other} onOpenChange={setOther} title="명령">
        <button onClick={() => setOther(false)}>다른 대화상자 닫기</button>
      </Dialog>
    </>
  );
}
test.each(["escape", "command"])(
  "a repeated native palette request is consumed while open and does not reopen after %s",
  async (close) => {
    const run = vi.fn();
    const release = useCommands.getState().register({ id: "owned.fixture", label: "소유 명령", run });
    render(<PaletteHost />);
    await screen.findByRole("dialog", { name: "명령" });
    act(() => useNativeRuntime.getState().push({ kind: "palette" }));
    if (close === "escape") await userEvent.keyboard("{Escape}");
    else await userEvent.click(screen.getByRole("option", { name: "소유 명령" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "명령" })).not.toBeInTheDocument());
    expect(useNativeRuntime.getState().intents).toHaveLength(0);
    expect(run).toHaveBeenCalledTimes(close === "command" ? 1 : 0);
    release();
  },
);
test("another dialog with the palette title still defers a native palette request until dismissed", async () => {
  render(<PaletteHost blocked />);
  await screen.findByRole("dialog", { name: "명령" });
  act(() => useNativeRuntime.getState().push({ kind: "palette" }));
  expect(useNativeRuntime.getState().intents).toHaveLength(1);
  expect(screen.queryByRole("combobox", { name: "명령 검색" })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "다른 대화상자 닫기" }));
  expect(await screen.findByRole("combobox", { name: "명령 검색" })).toBeVisible();
  expect(useNativeRuntime.getState().intents).toHaveLength(0);
});
