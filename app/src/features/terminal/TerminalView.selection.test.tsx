import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { registerTransformsNavigation, useTransformHandoff } from "../tools/transforms/handoff";
import { TerminalView } from "./TerminalView";

const mock = vi.hoisted(() => ({ selection: "", client: {}, focus: vi.fn(), dispose: vi.fn() }));
vi.mock("../../rpc/RpcProvider", () => ({ useRpc: () => mock.client }));
vi.mock("./useOsc7", () => ({ useOsc7: () => registerOsc }));
const registerOsc = () => ({ dispose: vi.fn() });
vi.mock("./initial-history", () => ({
  preserveInitialHistory: () => ({ dispose: vi.fn(), written: vi.fn(), reset: vi.fn() }),
}));
vi.mock("./link", () => ({
  TerminalLink: class {
    open = vi.fn();
    dispose = mock.dispose;
    resize = vi.fn();
    input = vi.fn();
  },
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options = { fontSize: 13, fontFamily: "", theme: {} };
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "" };
    parser = {};
    open = vi.fn();
    dispose = vi.fn();
    focus = mock.focus;
    loadAddon = vi.fn();
    attachCustomKeyEventHandler = vi.fn();
    hasSelection = () => mock.selection.length > 0;
    getSelection = () => mock.selection;
    onData = () => ({ dispose: vi.fn() });
    onBinary = this.onData;
    onResize = this.onData;
    onBell = this.onData;
    onTitleChange = this.onData;
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));
vi.mock("@xterm/addon-search", () => ({ SearchAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    onContextLoss = () => ({ dispose: vi.fn() });
  },
}));
it("opens only for an xterm selection, supports keyboard and sends its captured value", async () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const navigate = vi.fn();
  const stop = registerTransformsNavigation(navigate);
  const view = render(<TerminalView id="owned-selection" />);
  await act(async () => {
    await Promise.resolve();
  });
  const host = screen.getByRole("group", { name: "터미널 선택 작업" });
  mock.selection = "";
  fireEvent.contextMenu(host);
  expect(screen.queryByRole("menu")).toBeNull();
  mock.selection = "1700000000";
  host.focus();
  fireEvent.keyDown(host, { key: "F10", shiftKey: true });
  expect(screen.getByRole("menuitem")).toHaveFocus();
  mock.selection = "changed after open";
  fireEvent.click(screen.getByRole("menuitem"));
  expect(useTransformHandoff.getState().pending).toMatchObject({ text: "1700000000", source: "terminal" });
  expect(navigate).toHaveBeenCalledWith({ view: "transforms", tool: "timestamp" });
  view.unmount();
  expect(mock.dispose).toHaveBeenCalled();
  stop();
});
