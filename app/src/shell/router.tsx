import {
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  retainSearchParams,
  Link,
  type RouterHistory,
} from "@tanstack/react-router";
import type { ReactNode } from "react";
import { EmptyState } from "../ui";
import { SECTIONS } from "./sections";
const paths = new Set([...SECTIONS.map((section) => section.path), "/settings"]);
export function lastSection(): string {
  try {
    const path = localStorage.getItem("devbox.lastSection");
    return path && paths.has(path) ? path : "/agents";
  } catch {
    return "/agents";
  }
}
function remember(path: string) {
  try {
    localStorage.setItem("devbox.lastSection", path);
  } catch {
    /* Storage may be unavailable in private browsing. */
  }
}
export function buildRouter(root: () => ReactNode, settings: () => ReactNode, history?: RouterHistory) {
  const rootRoute = createRootRoute({
    component: root,
    validateSearch: (search: Record<string, unknown>): { gateway?: string } =>
      typeof search.gateway === "string" ? { gateway: search.gateway } : {},
    search: { middlewares: [retainSearchParams(["gateway"])] },
    notFoundComponent: () => (
      <EmptyState title="화면을 찾을 수 없습니다" action={<Link to="/agents">에이전트로 이동</Link>} />
    ),
  });
  const index = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    beforeLoad: ({ search }) => {
      throw redirect({ to: lastSection(), search });
    },
  });
  const sectionRoutes = SECTIONS.map((section) =>
    createRoute({
      getParentRoute: () => rootRoute,
      path: section.path,
      onEnter: () => remember(section.path),
      component: () => <EmptyState title={section.label} body="이 화면은 다음 단계에서 제공됩니다." />,
    }),
  );
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    onEnter: () => remember("/settings"),
    component: settings,
  });
  return createRouter({ routeTree: rootRoute.addChildren([index, ...sectionRoutes, settingsRoute]), history });
}
