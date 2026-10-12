import { fireEvent, render, screen } from "@testing-library/react";
import { SectionBoundary } from "./SectionBoundary";

test("section error keeps navigation, supports retry and route reset", () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  let broken = true;
  function Child() {
    if (broken) throw Error("broken");
    return <p>복구됨</p>;
  }
  const view = render(
    <>
      <button>섹션 이동</button>
      <SectionBoundary resetKey="one">
        <Child />
      </SectionBoundary>
    </>,
  );
  expect(screen.getByRole("button", { name: "섹션 이동" })).toBeEnabled();
  expect(screen.getByRole("alert")).toBeVisible();
  broken = false;
  fireEvent.click(screen.getByText("다시 시도"));
  expect(screen.getByText("복구됨")).toBeVisible();
  broken = true;
  view.rerender(
    <SectionBoundary resetKey="one">
      <Child />
    </SectionBoundary>,
  );
  broken = false;
  view.rerender(
    <SectionBoundary resetKey="two">
      <Child />
    </SectionBoundary>,
  );
  expect(screen.getByText("복구됨")).toBeVisible();
});

test("real frame activity navigation survives a throwing section", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const { createRootRoute, createRoute, createRouter, createMemoryHistory, RouterProvider } = await import(
    "@tanstack/react-router"
  );
  const { Frame } = await import("./Frame");
  const { TooltipProvider } = await import("../ui");
  function Broken() {
    throw Error("owned render failure");
    return null;
  }
  const root = createRootRoute({
    component: () => <Frame project={<span>프로젝트 미선택</span>} onOpenPalette={() => {}} />,
  });
  const broken = createRoute({ getParentRoute: () => root, path: "/agents", component: Broken });
  const next = createRoute({ getParentRoute: () => root, path: "/terminal", component: () => <p>터미널 본문</p> });
  const router = createRouter({
    routeTree: root.addChildren([broken, next]),
    history: createMemoryHistory({ initialEntries: ["/agents"] }),
  });
  render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  );
  await screen.findByText("화면을 표시하지 못했습니다");
  fireEvent.click(screen.getByRole("button", { name: "터미널" }));
  await screen.findByText("터미널 본문");
  expect(screen.getByRole("navigation", { name: "섹션" })).toBeVisible();
});
