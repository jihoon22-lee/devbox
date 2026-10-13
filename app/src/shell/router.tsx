import {
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  type RouterHistory,
  redirect,
  retainSearchParams,
  useRouterState,
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
export interface RouterParts {
  frame: () => ReactNode;
  pages: Partial<
    Record<
      string,
      { component: () => ReactNode; validateSearch?: (search: Record<string, unknown>) => Record<string, unknown> }
    >
  >;
  settings: () => ReactNode;
  popouts: { path: string; component: () => ReactNode }[];
}
function RouterFrame({ frame: FrameComponent }: { frame: () => ReactNode }) {
  const path = useRouterState({ select: (state) => state.location.pathname });
  return path.startsWith("/popout/") ? <Outlet /> : <FrameComponent />;
}
export function buildRouter(
  partsOrFrame: RouterParts | (() => ReactNode),
  settingsOrHistory?: (() => ReactNode) | RouterHistory,
  legacyHistory?: RouterHistory,
) {
  const parts: RouterParts =
    typeof partsOrFrame === "function"
      ? { frame: partsOrFrame, pages: {}, settings: settingsOrHistory as () => ReactNode, popouts: [] }
      : partsOrFrame;
  const history = typeof partsOrFrame === "function" ? legacyHistory : (settingsOrHistory as RouterHistory | undefined);
  const rootRoute = createRootRoute({
    component: () => <RouterFrame frame={parts.frame} />,
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
      validateSearch: parts.pages[section.path]?.validateSearch,
      component:
        parts.pages[section.path]?.component ??
        (() => <EmptyState title={section.label} body="이 화면은 다음 단계에서 제공됩니다." />),
    }),
  );
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    onEnter: () => remember("/settings"),
    component: parts.settings,
  });
  const popoutRoutes = parts.popouts.map((popout) =>
    createRoute({ getParentRoute: () => rootRoute, path: popout.path, component: popout.component }),
  );
  return createRouter({
    routeTree: rootRoute.addChildren([index, ...sectionRoutes, settingsRoute, ...popoutRoutes]),
    history,
  });
}
