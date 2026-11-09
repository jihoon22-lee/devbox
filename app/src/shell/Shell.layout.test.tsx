import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectWire, projects } from "../features/projects/test-wire";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider } from "../ui";
import { useToasts } from "../ui/Toast";
import { useCommands } from "./commands";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

beforeEach(() => {
  localStorage.clear();
  useLayout.setState({ listWidth: 260, listCollapsed: false, listAutoCollapsed: false, bottomOpen: false });
});
// jsdom has no ResizeObserver, so the frame follows the window size.
function viewport(width: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
}
function resize(width: number) {
  act(() => {
    viewport(width);
    window.dispatchEvent(new Event("resize"));
  });
}
async function agents(width: number) {
  viewport(width);
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  window.history.replaceState({}, "", "/agents");
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
}
const list = () => screen.queryByRole("complementary", { name: "목록" });
const expand = () => screen.queryByRole("button", { name: "목록 펼치기" });
const saved = () => JSON.parse(localStorage.getItem("devbox.layout") ?? "{}").state;
const toggleKey = () => fireEvent.keyDown(window, { code: "KeyB", ctrlKey: true, shiftKey: true });

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
  expect(items.find((t) => t.id === "layout.list")).toMatchObject({
    label: "목록 접기·펴기",
    keys: ["Ctrl", "Shift", "B"],
  });
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

test("목록이 접히면 '목록 펼치기' 단추가 보이고, 누르면 목록이 열린다", async () => {
  await agents(1280);
  const aside = list();
  expect(aside).toBeInTheDocument();
  expect(expand()).toBeNull();
  act(() => useLayout.setState({ listCollapsed: true }));
  expect(list()).toBeNull();
  const button = expand()!;
  expect(button).toHaveAttribute("aria-expanded", "false");
  expect(button).toHaveAttribute("aria-controls", aside!.id);
  act(() => button.focus());
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Ctrl+Shift+B");
  await userEvent.click(button);
  expect(useLayout.getState().listCollapsed).toBe(false);
  expect(list()).toBe(aside);
  expect(expand()).toBeNull();
  // The button is gone, so focus goes to the list it opened instead of the page.
  expect(aside).toHaveFocus();
});

test("자동으로 접힌 목록은 창이 넓어지면 다시 열리고 저장되지 않는다", async () => {
  await agents(1000);
  expect(list()).toBeNull();
  expect(saved()).toMatchObject({ listCollapsed: false });
  expect(saved()).not.toHaveProperty("listAutoCollapsed");
  resize(1200);
  expect(list()).toBeInTheDocument();
  expect(saved()).toMatchObject({ listCollapsed: false });
  resize(1000);
  expect(expand()).toBeInTheDocument();
  resize(1200);
  expect(expand()).toBeNull();
  // Narrowing again collapses again; opening it while narrow keeps it open until the next narrowing.
  resize(1000);
  expect(list()).toBeNull();
  toggleKey();
  expect(list()).toBeInTheDocument();
  resize(1050);
  expect(list()).toBeInTheDocument();
  expect(saved()).toMatchObject({ listCollapsed: false });
});

test("사용자가 접은 목록은 창이 넓어져도 접힌 채다", async () => {
  await agents(1200);
  toggleKey();
  expect(list()).toBeNull();
  resize(1000);
  resize(1200);
  expect(list()).toBeNull();
  expect(expand()).toBeInTheDocument();
  expect(saved()).toMatchObject({ listCollapsed: true });
});

test("직접 접은 뒤 창이 좁아져도 '목록 펼치기' 한 번에 목록이 열린다", async () => {
  await agents(1200);
  toggleKey();
  resize(1000);
  await userEvent.click(expand()!);
  expect(list()).toBeInTheDocument();
  expect(saved()).toMatchObject({ listCollapsed: false });
  resize(1200);
  expect(list()).toBeInTheDocument();
});

test("자동으로 접힐 때 목록 안의 포커스는 '목록 펼치기'로 옮긴다", async () => {
  await agents(1200);
  const grid = within(list()!).getByRole("button", { name: "격자" });
  act(() => grid.focus());
  resize(1000);
  expect(expand()).toHaveFocus();
  // Widening removes the focused button; focus goes into the list it opened.
  resize(1200);
  expect(list()).toHaveFocus();
  const edge = screen.getByRole("separator", { name: "목록 너비" });
  act(() => edge.focus());
  resize(1000);
  expect(expand()).toHaveFocus();
  // A collapse the user asks for moves focus the same way, and Ctrl+Shift+B brings it back.
  await userEvent.click(expand()!);
  act(() => grid.focus());
  toggleKey();
  expect(expand()).toHaveFocus();
  toggleKey();
  expect(list()).toHaveFocus();
});

test("'목록 펼치기'에서 Ctrl+Shift+B로 목록을 열면 포커스가 목록으로 간다", async () => {
  await agents(1200);
  act(() => within(list()!).getByRole("button", { name: "격자" }).focus());
  resize(1000);
  expect(expand()).toHaveFocus();
  toggleKey();
  expect(list()).toHaveFocus();
});
