import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useToasts } from "../ui/Toast";
import { useCommands } from "./commands";
import { Shell } from "./Shell";

test("Shell registers layout commands and owns mouse history with one shared toast host", async () => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  window.history.replaceState({}, "", "/agents");
  const wire = new ProjectWire();
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
  const items = useCommands.getState().list();
  expect(items.find((t) => t.id === "layout.list")?.keys).toEqual(["Ctrl", "Shift", "B"]);
  expect(items.find((t) => t.id === "layout.bottom")?.keys).toEqual(["Ctrl", "`"]);
  await userEvent.click(screen.getByRole("button", { name: "터미널" }));
  await waitFor(() => expect(window.location.pathname).toBe("/terminal"));
  await userEvent.click(screen.getByRole("button", { name: "노트" }));
  await waitFor(() => expect(window.location.pathname).toBe("/notes"));
  fireEvent.mouseUp(window, { button: 3 });
  await waitFor(() => expect(window.location.pathname).toBe("/terminal"));
  fireEvent.mouseUp(window, { button: 4 });
  await waitFor(() => expect(window.location.pathname).toBe("/notes"));
  let id = 0;
  act(() => {
    id = useToasts.getState().push({ tone: "info", title: "공통 알림" });
  });
  expect(screen.getAllByRole("region", { name: "알림" })).toHaveLength(1);
  expect(screen.getByText("공통 알림")).toBeInTheDocument();
  act(() => useToasts.getState().dismiss(id));
  view.unmount();
  expect(
    useCommands
      .getState()
      .list()
      .some((t) => t.id === "layout.list"),
  ).toBe(false);
});
