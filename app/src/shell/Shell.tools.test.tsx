import { act, render, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ProjectWire } from "../features/projects/test-wire";
import { useTransformHandoff } from "../features/tools/transforms/handoff";
import { RpcProvider } from "../rpc/RpcProvider";
import { TooltipProvider, useToasts } from "../ui";
import { useCommands } from "./commands";
import { useLayout } from "./layout";
import { Shell } from "./Shell";

vi.mock("../features/terminal/TerminalView", () => ({ TerminalView: () => <div /> }));
vi.mock("./router", async (importOriginal) => {
  const original = await importOriginal<typeof import("./router")>();
  return {
    ...original,
    buildRouter: (...args: Parameters<typeof original.buildRouter>) => {
      const router = original.buildRouter(...args);
      const navigate = router.navigate;
      router.navigate = (options) => {
        if (options.to === "/tools") {
          const rejected = Promise.reject(new Error("private router diagnostic"));
          // Keep the RED assertion focused on Shell dropping the promise.
          void rejected.catch(() => {});
          return rejected;
        }
        return navigate(options);
      };
      return router;
    },
  };
});
it("Shell forwards the navigation rejection to handoff cleanup and a fixed toast", async () => {
  window.history.replaceState({}, "", "/notes");
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  useLayout.setState({ bottomOpen: false });
  useToasts.setState({ items: [] });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { readText: vi.fn().mockResolvedValue('{"private":"fixture"}') },
  });
  const wire = new ProjectWire({ trustNotRequired: true });
  const view = render(
    <RpcProvider client={wire.client}>
      <TooltipProvider>
        <Shell />
      </TooltipProvider>
    </RpcProvider>,
  );
  await waitFor(() => expect(useCommands.getState().items.has("tools.clipboard")).toBe(true));
  act(() => useCommands.getState().items.get("tools.clipboard")?.run());
  await waitFor(() =>
    expect(useToasts.getState().items.some((item) => item.title === "변환 도구를 열지 못했습니다.")).toBe(true),
  );
  expect(useTransformHandoff.getState().pending).toBeNull();
  expect(JSON.stringify(useToasts.getState().items)).not.toContain("private router diagnostic");
  view.unmount();
  window.history.replaceState({}, "", "/");
});
