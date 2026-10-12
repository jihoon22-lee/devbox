import { createMemoryHistory, Outlet, RouterProvider } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import { buildRouter, lastSection } from "./router";

test("last section is allowlisted with storage failure fallback", () => {
  localStorage.setItem("devbox.lastSection", "/settings");
  expect(lastSection()).toBe("/settings");
  localStorage.setItem("devbox.lastSection", "https://evil.test");
  expect(lastSection()).toBe("/agents");
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw Error();
  });
  expect(lastSection()).toBe("/agents");
});
test("index restores settings and preserves gateway across navigation", async () => {
  localStorage.setItem("devbox.lastSection", "/settings");
  const router = buildRouter(
    () => <Outlet />,
    () => <p>설정 페이지</p>,
    createMemoryHistory({ initialEntries: ["/?gateway=ws%3A%2F%2Flocalhost%3A1451%2F%3Ftoken%3Downed"] }),
  );
  render(<RouterProvider router={router} />);
  await screen.findByText("설정 페이지");
  await router.navigate({ to: "/notes" });
  await waitFor(() => expect(router.state.location.pathname).toBe("/notes"));
  expect(router.state.location.search).toEqual({ gateway: "ws://localhost:1451/?token=owned" });
  expect(localStorage.getItem("devbox.lastSection")).toBe("/notes");
});
test("unknown routes provide a usable recovery link despite unavailable storage", async () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw Error("storage unavailable");
  });
  const router = buildRouter(
    () => <Outlet />,
    () => <p>설정 페이지</p>,
    createMemoryHistory({ initialEntries: ["/missing"] }),
  );
  render(<RouterProvider router={router} />);
  const link = await screen.findByRole("link", { name: "에이전트로 이동" });
  link.click();
  await screen.findByText("에이전트");
});

beforeEach(() => vi.spyOn(window, "scrollTo").mockImplementation(() => {}));
