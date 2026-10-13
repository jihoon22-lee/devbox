import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { buildRouter } from "../../shell/router";
import { NotificationCenter, relativeTime } from "./NotificationCenter";
import { useNotifications } from "./store";

afterEach(() => {
  cleanup();
  useNotifications.getState().clear();
});
function setup() {
  const router = buildRouter(
    { frame: () => <NotificationCenter />, pages: {}, settings: () => null, popouts: [] },
    createMemoryHistory({ initialEntries: ["/notes?gateway=ws%3A%2F%2Ffixture&renderer=dom"] }),
  );
  render(<RouterProvider router={router} />);
  return router;
}
test("bell opens a keyboard popover; target activation reads only its notice, closes and preserves connection", async () => {
  act(() => {
    useNotifications.getState().push({
      key: "a",
      kind: "agent",
      title: "작업 A",
      body: "needs Bash",
      target: { to: "/agents", search: { id: "a", view: "list" } },
    });
    useNotifications.getState().push({ key: "b", kind: "error", title: "오류", body: "details" });
  });
  const router = setup();
  const bell = await screen.findByRole("button", { name: "알림 2개" });
  act(() => bell.focus());
  await userEvent.keyboard("{Enter}");
  expect(await screen.findByRole("dialog")).toHaveTextContent("needs Bash");
  expect(screen.getAllByText("방금")).toHaveLength(2);
  await userEvent.click(screen.getByRole("button", { name: /작업 A/ }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/agents"));
  expect(router.state.location.search).toMatchObject({
    id: "a",
    view: "list",
    gateway: "ws://fixture",
    renderer: "dom",
  });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(useNotifications.getState().unread).toBe(1);
});
test("all read and clear keep the center open; escape closes and returns focus", async () => {
  useNotifications.getState().push({ key: "a", kind: "error", title: "오류", body: "" });
  setup();
  const bell = await screen.findByRole("button", { name: "알림 1개" });
  await userEvent.click(bell);
  fireEvent.click(await screen.findByRole("button", { name: "모두 읽음" }));
  expect(useNotifications.getState().unread).toBe(0);
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "비우기" }));
  expect(screen.getByText("알림이 없습니다")).toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(bell).toHaveFocus();
});
test("relative times clamp future timestamps and display minute, hour and day", () => {
  expect(relativeTime(2, 1)).toBe("방금");
  expect(relativeTime(0, 120_000)).toBe("2분 전");
  expect(relativeTime(0, 3600_000)).toBe("1시간 전");
  expect(relativeTime(0, 86400_000)).toBe("1일 전");
});
