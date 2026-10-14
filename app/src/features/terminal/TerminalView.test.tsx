import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TooltipProvider, useToasts } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { setTerminalFont } from "../settings/preferences";
import { useTerminalFocus } from "./focus";
import { emptyLayout, openTab, splitActive, useTerminalLayout } from "./store";
import { TerminalPage } from "./TerminalPage";
import { TerminalView } from "./TerminalView";

const mock = vi.hoisted(() => ({
  terms: [] as any[],
  links: [] as any[],
  fits: [] as any[],
  searches: [] as any[],
  webgl: [] as any[],
  callbacks: [] as (() => void)[],
  client: {},
  metadata: { data: undefined as { hostname?: string } | undefined, isSuccess: false, isError: false },
  create: vi.fn(async () => ({ id: "s2" })),
  openError: false,
  webglError: false,
}));
vi.mock("../../rpc/RpcProvider", () => ({ useRpc: () => mock.client }));
vi.mock("../../rpc/query", () => ({
  useRpcMutation: () => ({ isPending: false }),
  useRpcQuery: (method: string) => (method === "system.info" ? mock.metadata : { data: { items: [] } }),
  confirmPolicy: () => "always",
}));
vi.mock("./useSessions", () => {
  const items = ["s1", "s2", "s3"].map((id) => ({
    id,
    projectId: null,
    title: id,
    cwd: "/fixture",
    command: "bash",
    alive: true,
    kind: "shell",
    createdMs: 1,
  }));
  return {
    useSessions: () => ({ data: { rev: "1", items }, items, isError: false, isFetching: false }),
    useCreateSession: () => mock.create,
  };
});
vi.mock("./link", () => ({
  TerminalLink: class {
    open = vi.fn(async () => this.options.onState("live"));
    input = vi.fn();
    resize = vi.fn();
    dispose = vi.fn();
    constructor(
      public client: unknown,
      public options: any,
    ) {
      mock.links.push(this);
    }
  },
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "" };
    options: any;
    events: Record<string, Function> = {};
    parser = {
      registerCsiHandler: (_: unknown, cb: Function) => {
        this.events.csi = cb;
        return { dispose: vi.fn() };
      },
      registerOscHandler: (_: number, cb: Function) => {
        this.events.osc = cb;
        return { dispose: vi.fn() };
      },
    };
    open = vi.fn(() => {
      if (mock.openError) throw new Error("open failed");
    });
    dispose = vi.fn();
    focus = vi.fn();
    write = vi.fn();
    reset = vi.fn();
    paste = vi.fn();
    resize = vi.fn((c: number, r: number) => {
      this.cols = c;
      this.rows = r;
    });
    hasSelection = () => true;
    getSelection = () => "selection";
    loadAddon = vi.fn();
    attachCustomKeyEventHandler = (cb: Function) => {
      this.events.key = cb;
    };
    constructor(options: any) {
      this.options = options;
      mock.terms.push(this);
    }
    onData = (cb: Function) => this.listen("data", cb);
    onBinary = (cb: Function) => this.listen("binary", cb);
    onResize = (cb: Function) => this.listen("resize", cb);
    onBell = (cb: Function) => this.listen("bell", cb);
    onTitleChange = (cb: Function) => this.listen("title", cb);
    listen(name: string, cb: Function) {
      this.events[name] = cb;
      return { dispose: vi.fn() };
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
    constructor() {
      mock.fits.push(this);
    }
  },
}));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn();
    findPrevious = vi.fn();
    clearDecorations = vi.fn();
    constructor() {
      mock.searches.push(this);
    }
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    dispose = vi.fn();
    onContextLoss = (cb: () => void) => {
      mock.callbacks.push(cb);
      return { dispose: vi.fn() };
    };
    constructor() {
      if (mock.webglError) throw new Error("no WebGL");
      mock.webgl.push(this);
    }
  },
}));
const flush = async () => {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};
test("selecting an existing split session focuses its real view, including selecting the same session again", async () => {
  useCurrentProject.getState().set(undefined);
  useTerminalLayout.setState({ byProject: { none: splitActive(openTab(emptyLayout(), "s1"), "s1", "s2", "row") } });
  useTerminalFocus.setState({ byProject: { none: "s1" }, cwd: {}, bells: {} });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TooltipProvider>
        <TerminalPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  await flush();
  const first = mock.terms[0],
    second = mock.terms[1];
  expect(first.focus).toHaveBeenCalledTimes(1);
  expect(second.focus).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: /s2.*bash/ }));
  expect(second.focus).toHaveBeenCalledTimes(1);
  await userEvent.click(screen.getByRole("button", { name: /s2.*bash/ }));
  expect(second.focus).toHaveBeenCalledTimes(2);
  act(() => second.events.key(new KeyboardEvent("keydown", { code: "KeyF", ctrlKey: true, shiftKey: true })));
  const search = screen.getByRole("textbox", { name: "터미널 검색" });
  expect(search).toHaveFocus();
  act(() => useTerminalFocus.getState().ring("s2"));
  expect(second.focus).toHaveBeenCalledTimes(2);
  expect(search).toHaveFocus();
  expect(mock.terms).toHaveLength(2);
  expect(mock.links).toHaveLength(2);
  expect(first.dispose).not.toHaveBeenCalled();
  expect(second.dispose).not.toHaveBeenCalled();
});
test("directly focusing another pane supersedes an old selection request when its tab remounts", async () => {
  useCurrentProject.getState().set(undefined);
  const split = splitActive(openTab(emptyLayout(), "s1"), "s1", "s2", "row");
  const layout = { ...openTab(split, "s3"), active: split.active };
  useTerminalLayout.setState({ byProject: { none: layout } });
  useTerminalFocus.setState({ byProject: { none: "s1" }, cwd: {}, bells: {} });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TooltipProvider>
        <TerminalPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  await flush();
  await userEvent.click(screen.getByRole("button", { name: /s2.*bash/ }));
  expect(mock.terms[1].focus).toHaveBeenCalledTimes(1);
  // The renderer fixture has no textarea; add one to exercise real browser focus
  // capture through the actual PaneTree, rather than updating the focus store.
  const input = document.createElement("textarea");
  document.querySelector('[data-terminal-session="s1"]')!.append(input);
  act(() => input.focus());
  expect(useTerminalFocus.getState().byProject.none).toBe("s1");
  await userEvent.click(screen.getByRole("tab", { name: "s3" }));
  await flush();
  await userEvent.click(screen.getByRole("tab", { name: "s1 +1" }));
  await flush();
  expect(mock.terms).toHaveLength(5);
  expect(mock.terms[3].focus).toHaveBeenCalledTimes(1);
  expect(mock.terms[4].focus).not.toHaveBeenCalled();
  expect(useTerminalFocus.getState().byProject.none).toBe("s1");
});
beforeEach(() => {
  for (const list of [mock.terms, mock.links, mock.fits, mock.searches, mock.webgl, mock.callbacks]) list.length = 0;
  mock.metadata = { data: { hostname: "local-wsl" }, isSuccess: true, isError: false };
  mock.create.mockClear();
  mock.openError = false;
  mock.webglError = false;
  for (const item of useToasts.getState().items) useToasts.getState().dismiss(item.id);
  localStorage.clear();
  window.history.replaceState({}, "", "/");
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe = vi.fn();
      disconnect = vi.fn();
    },
  );
  Object.defineProperty(document, "fonts", { configurable: true, value: { load: vi.fn().mockResolvedValue([]) } });
});
afterEach(() => vi.unstubAllGlobals());
test("callbacks update without remount, raw binary remains bytes and cleanup owns one link", async () => {
  const first = vi.fn(),
    next = vi.fn();
  const view = render(<TerminalView id="t1" onBell={first} />);
  await flush();
  const term = mock.terms[0];
  view.rerender(<TerminalView id="t1" onBell={next} />);
  act(() => {
    term.events.bell();
    term.events.binary(String.fromCharCode(128, 255));
  });
  expect(first).not.toHaveBeenCalled();
  expect(next).toHaveBeenCalledOnce();
  expect(mock.terms).toHaveLength(1);
  expect(mock.links[0].input).toHaveBeenCalledWith(new Uint8Array([128, 255]));
  view.unmount();
  expect(mock.links[0].dispose).toHaveBeenCalledOnce();
  expect(term.dispose).toHaveBeenCalledOnce();
});
test("read-only scaled view adopts attach grid without fit/input/paste/upstream resize", async () => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { readText: vi.fn().mockResolvedValue("secret") },
  });
  render(<TerminalView id="t1" readOnly scale={0.5} />);
  await flush();
  const term = mock.terms[0],
    link = mock.links[0];
  act(() => {
    link.options.sink.resize(132, 43);
    term.events.data("a");
    term.events.binary("b");
    term.events.resize({ cols: 20, rows: 10 });
    term.events.key(new KeyboardEvent("keydown", { code: "KeyV", ctrlKey: true, shiftKey: true }));
  });
  await flush();
  expect(term.resize).toHaveBeenCalledWith(132, 43);
  expect(mock.fits[0].fit).not.toHaveBeenCalled();
  expect(link.input).not.toHaveBeenCalled();
  expect(link.resize).not.toHaveBeenCalled();
  expect(navigator.clipboard.readText).not.toHaveBeenCalled();
});
test("font load completing after teardown never opens or attaches a disposed terminal", async () => {
  let resolve!: () => void;
  document.fonts.load = vi.fn(
    () =>
      new Promise<FontFace[]>((r) => {
        resolve = () => r([]);
      }),
  );
  const view = render(<TerminalView id="t1" />);
  await flush();
  view.unmount();
  resolve();
  await flush();
  expect(mock.terms[0].open).not.toHaveBeenCalled();
  expect(mock.links[0].open).not.toHaveBeenCalled();
});
test("DOM option skips WebGL and terminal search owns focus/Enter/Shift+Enter/Escape", async () => {
  window.history.replaceState({}, "", "/?renderer=dom");
  render(<TerminalView id="t1" />);
  await flush();
  expect(mock.webgl).toHaveLength(0);
  act(() => mock.terms[0].events.key(new KeyboardEvent("keydown", { code: "KeyF", ctrlKey: true, shiftKey: true })));
  const input = screen.getByRole("textbox", { name: "터미널 검색" });
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "row" } });
  fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
  expect(mock.searches[0].findPrevious).toHaveBeenCalledWith("row");
  fireEvent.keyDown(input, { key: "Enter" });
  expect(mock.searches[0].findNext).toHaveBeenCalledWith("row");
  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(mock.terms[0].focus).toHaveBeenCalled();
});

test("font timeout opens fallback and late font readiness refits only a live view", async () => {
  vi.useFakeTimers();
  let resolve!: () => void;
  document.fonts.load = vi.fn(
    () =>
      new Promise<FontFace[]>((r) => {
        resolve = () => r([]);
      }),
  );
  const view = render(<TerminalView id="t1" />);
  await flush();
  expect(mock.terms[0].open).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1500);
  });
  expect(mock.terms[0].open).toHaveBeenCalledOnce();
  const fits = mock.fits[0].fit.mock.calls.length;
  resolve();
  await flush();
  expect(mock.fits[0].fit).toHaveBeenCalledTimes(fits + 1);
  view.unmount();
  vi.useRealTimers();
});
test("late clipboard paste never reaches an unmounted view and rejected clipboard reports a toast", async () => {
  let resolve!: (text: string) => void;
  const readText = vi.fn(
    () =>
      new Promise<string>((r) => {
        resolve = r;
      }),
  );
  const writeText = vi.fn().mockRejectedValue(new Error("denied"));
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { readText, writeText } });
  const view = render(<TerminalView id="t1" />);
  await flush();
  const term = mock.terms[0];
  act(() => term.events.key(new KeyboardEvent("keydown", { code: "KeyC", ctrlKey: true, shiftKey: true })));
  await flush();
  expect(useToasts.getState().items.some((item) => item.title === "클립보드에 복사하지 못했습니다")).toBe(true);
  act(() => term.events.key(new KeyboardEvent("keydown", { code: "KeyV", ctrlKey: true, shiftKey: true })));
  await flush();
  view.unmount();
  resolve("secret");
  await flush();
  expect(term.paste).not.toHaveBeenCalled();
  expect(writeText).toHaveBeenCalledWith("selection");
});
test("scale changes preserve one renderer/link and WebGL context loss disposes its addon", async () => {
  const view = render(<TerminalView id="t1" readOnly scale={0.5} />);
  await flush();
  view.rerender(<TerminalView id="t1" readOnly scale={0.25} />);
  expect(mock.terms).toHaveLength(1);
  expect(mock.links).toHaveLength(1);
  act(() => mock.callbacks[0]!());
  expect(mock.webgl[0].dispose).toHaveBeenCalledOnce();
});
test("invalid/blocked font preferences remain usable", async () => {
  localStorage.setItem("devbox.terminalFont", "NaN");
  const view = render(<TerminalView id="t1" />);
  await flush();
  expect(mock.terms[0].options.fontSize).toBe(13);
  view.unmount();
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("denied");
  });
  render(<TerminalView id="t2" />);
  await flush();
  expect(mock.terms[1].options.fontSize).toBe(13);
});

test("changing session while its font loads disposes the old link and only opens the new session", async () => {
  const resolves: (() => void)[] = [];
  document.fonts.load = vi.fn(() => new Promise<FontFace[]>((r) => resolves.push(() => r([]))));
  const view = render(<TerminalView id="t1" />);
  await flush();
  view.rerender(<TerminalView id="t2" />);
  await flush();
  resolves[0]!();
  resolves[1]!();
  await flush();
  expect(mock.links[0].dispose).toHaveBeenCalledOnce();
  expect(mock.links[0].open).not.toHaveBeenCalled();
  expect(mock.links[1].open).toHaveBeenCalledOnce();
  expect(mock.terms[0].open).not.toHaveBeenCalled();
});
test("renderer initialization failure shows lost state and tears down owned resources once", async () => {
  mock.openError = true;
  const view = render(<TerminalView id="t1" />);
  await flush();
  expect(screen.getByRole("status")).toHaveTextContent("다시 연결 중 · 입력 전송 안 됨");
  expect(mock.links[0].dispose).toHaveBeenCalledOnce();
  expect(mock.links[0].open).not.toHaveBeenCalled();
  expect(mock.terms[0].dispose).toHaveBeenCalledOnce();
  view.unmount();
  expect(mock.terms[0].dispose).toHaveBeenCalledOnce();
});
test("WebGL construction failure falls back to the opened DOM terminal", async () => {
  mock.webglError = true;
  render(<TerminalView id="t1" />);
  await flush();
  expect(mock.terms[0].open).toHaveBeenCalledOnce();
  expect(mock.links[0].open).toHaveBeenCalledOnce();
  expect(screen.queryByRole("status")).toBeNull();
});

test("settings font preference updates all live views while terminal zoom stays local", async () => {
  render(
    <>
      <TerminalView id="font-a" />
      <TerminalView id="font-b" />
    </>,
  );
  await flush();
  const [first, second] = mock.terms.slice(-2);
  act(() => setTerminalFont(18));
  expect(first.options.fontSize).toBe(18);
  expect(second.options.fontSize).toBe(18);
  const key = new KeyboardEvent("keydown", { code: "Equal", key: "+", ctrlKey: true, cancelable: true });
  act(() => first.events.key(key));
  expect(first.options.fontSize).toBe(19);
  expect(second.options.fontSize).toBe(18);
  expect(localStorage.getItem("devbox.terminalFont")).toBe("19");
});

test("readable old font storage cannot override the live or newly opened font after a quota failure", async () => {
  localStorage.setItem("devbox.terminalFont", "13");
  const view = render(<TerminalView id="quota-a" />);
  await flush();
  const first = mock.terms.at(-1);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  act(() => setTerminalFont(22));
  expect(first.options.fontSize).toBe(22);
  view.rerender(
    <>
      <TerminalView id="quota-a" />
      <TerminalView id="quota-b" />
    </>,
  );
  await flush();
  expect(mock.terms.at(-1).options.fontSize).toBe(22);
  expect(localStorage.getItem("devbox.terminalFont")).toBe("13");
});

test.each(["KeyT", "KeyE"])("remote OSC cannot choose a local new tab/split cwd (%s)", async (code) => {
  useCurrentProject.getState().set(undefined);
  useTerminalLayout.setState({ byProject: { none: openTab(emptyLayout(), "s1") } });
  useTerminalFocus.setState({ byProject: { none: "s1" }, cwd: {}, bells: {} });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TooltipProvider>
        <TerminalPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  await flush();
  act(() => {
    mock.terms[0].events.osc("file://local-wsl/local/safe");
    mock.terms[0].events.osc("file://remote-host/remote-but-existing-local");
  });
  fireEvent.keyDown(window, { code, ctrlKey: true, shiftKey: true });
  await flush();
  expect(mock.create).toHaveBeenCalledWith({
    projectId: null,
    cwd: "/local/safe",
    title: null,
    command: null,
    attachZellij: null,
  });
});
test("pending local OSC uses arriving hostname and the latest callback without remount", async () => {
  mock.metadata = { data: undefined, isSuccess: false, isError: false };
  const first = vi.fn(),
    next = vi.fn();
  const view = render(<TerminalView id="pending" onCwd={first} />);
  await flush();
  act(() => mock.terms[0].events.osc("file://local-wsl/first%20prompt"));
  expect(first).not.toHaveBeenCalled();
  view.rerender(<TerminalView id="pending" onCwd={next} />);
  mock.metadata = { data: { hostname: "LOCAL-WSL." }, isSuccess: true, isError: false };
  view.rerender(<TerminalView id="pending" onCwd={next} />);
  await flush();
  expect(first).not.toHaveBeenCalled();
  expect(next).toHaveBeenCalledExactlyOnceWith("/first prompt");
  expect(mock.terms).toHaveLength(1);
  expect(mock.links).toHaveLength(1);
});
test.each(["file://remote-host/new", "not a URL"])(
  "latest invalid or remote candidate supersedes pending local OSC (%s)",
  async (candidate) => {
    mock.metadata = { data: undefined, isSuccess: false, isError: false };
    const cwd = vi.fn();
    const view = render(<TerminalView id="latest" onCwd={cwd} />);
    await flush();
    act(() => {
      mock.terms[0].events.osc("file://local-wsl/old");
      mock.terms[0].events.osc(candidate);
    });
    expect(cwd).not.toHaveBeenCalled();
    mock.metadata = { data: { hostname: "local-wsl" }, isSuccess: true, isError: false };
    view.rerender(<TerminalView id="latest" onCwd={cwd} />);
    await flush();
    expect(cwd).not.toHaveBeenCalled();
    act(() => mock.terms[0].events.osc("file://local-wsl/current"));
    expect(cwd).toHaveBeenCalledExactlyOnceWith("/current");
  },
);
test("pending OSC cannot cross session replacement or disposed renderer lifetime", async () => {
  mock.metadata = { data: undefined, isSuccess: false, isError: false };
  const oldCwd = vi.fn(),
    newCwd = vi.fn();
  const view = render(<TerminalView id="old" onCwd={oldCwd} />);
  await flush();
  const old = mock.terms[0];
  act(() => old.events.osc("file://local-wsl/old"));
  mock.metadata = { data: { hostname: "local-wsl" }, isSuccess: true, isError: false };
  view.rerender(<TerminalView id="new" onCwd={newCwd} />);
  await flush();
  expect(oldCwd).not.toHaveBeenCalled();
  expect(newCwd).not.toHaveBeenCalled();
  const current = mock.terms[1];
  act(() => {
    old.events.osc("file://local-wsl/stale");
    current.events.osc("file://local-wsl/current");
  });
  expect(newCwd).toHaveBeenCalledExactlyOnceWith("/current");
  view.unmount();
  act(() => current.events.osc("file:///disposed"));
  expect(newCwd).toHaveBeenCalledOnce();
});
test.each([false, true])("missing or failed hostname keeps named paths unknown (%s)", async (failed) => {
  mock.metadata = { data: failed ? undefined : {}, isSuccess: !failed, isError: failed };
  const cwd = vi.fn();
  render(<TerminalView id="unknown" onCwd={cwd} />);
  await flush();
  act(() => {
    mock.terms[0].events.osc("file://local-wsl/unknown");
    mock.terms[0].events.osc("file:///hostless");
    mock.terms[0].events.osc("file://localhost/localhost");
  });
  expect(cwd.mock.calls).toEqual([["/hostless"], ["/localhost"]]);
});
