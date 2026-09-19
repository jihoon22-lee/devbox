import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useEffect, type CSSProperties } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PaneCanvas from "./PaneCanvas";
import type { Pane, Tab } from "../types";
import { DEFAULT_SETTINGS, TERMINAL_THEMES, fontFamilyFor } from "../lib/settings";
import { normalizePaneSizing } from "../lib/paneSizing";

/**
 * TermPane을 모킹해 마운트/언마운트를 스파이로 잡는다. 실제 xterm은 jsdom에서 돌지
 * 않으므로(터미널 렌더링에 canvas/measureText 등이 필요) TermPane 자체를 갈아치운다 —
 * 이러면 실제 TermPane 모듈(과 그 안의 @xterm/xterm, ../api import)이 아예 평가되지
 * 않으므로 별도로 mocking할 필요가 없다.
 */
const { mountSpy, unmountSpy, termPropsSpy } = vi.hoisted(() => ({
  mountSpy: vi.fn<(sessionId: string) => void>(),
  unmountSpy: vi.fn<(sessionId: string) => void>(),
  termPropsSpy: vi.fn<(sessionId: string, initialCommand: string | undefined) => void>(),
}));

vi.mock("./TermPane", () => ({
  default: (props: { sessionId: string; active: boolean; initialCommand?: string; style?: CSSProperties }) => {
    termPropsSpy(props.sessionId, props.initialCommand);
    useEffect(() => {
      mountSpy(props.sessionId);
      return () => unmountSpy(props.sessionId);
      // eslint 없음(레포에 eslint 설정이 없다) — sessionId만 의존성으로 둔다.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.sessionId]);
    return (
      <div
        data-testid={`pane-${props.sessionId}`}
        data-active={String(props.active)}
        style={props.style}
      >
        {props.sessionId}
      </div>
    );
  },
}));

// key와 sessionId를 같은 값으로 둔다 — 이 테스트 스위트는 세션 재부착이 아니라
// panes 배열 순서/재마운트 방지 불변식을 검증하는 것이 목적이라 둘을 구분할
// 필요가 없다. 기존 테스트의 "p1"/"p2" 참조는 그대로 sessionId로도 동작한다.
function pane(id: string, distro = "Ubuntu"): Pane {
  return { key: id, sessionId: id, distro, multiplexer: "native" };
}

function tab(id: string, paneIds: string[]): Tab {
  return {
    id,
    title: id,
    layout: "grid",
    paneIds,
    sizing: normalizePaneSizing(undefined, "grid", paneIds.length),
  };
}

function baseProps(overrides: Partial<Parameters<typeof PaneCanvas>[0]> = {}) {
  return {
    tabs: [] as Tab[],
    panes: [] as Pane[],
    activeTabId: "",
    activePaneId: null as string | null,
    broadcastOn: false,
    broadcastTargetIds: [] as string[],
    copyOnSelect: true,
    fontSize: 13,
    fontFamily: fontFamilyFor(DEFAULT_SETTINGS.fontId),
    theme: TERMINAL_THEMES[DEFAULT_SETTINGS.theme],
    cursorStyle: DEFAULT_SETTINGS.cursorStyle,
    cursorBlink: DEFAULT_SETTINGS.cursorBlink,
    scrollbackLines: DEFAULT_SETTINGS.scrollbackLines,
    registerWrite: vi.fn(),
    unregisterWrite: vi.fn(),
    registerFocus: vi.fn(),
    unregisterFocus: vi.fn(),
    registerTerminalHandle: vi.fn(),
    unregisterTerminalHandle: vi.fn(),
    onClosePane: vi.fn(),
    onRetryPane: vi.fn(),
    onFocusPane: vi.fn(),
    onSizingChange: vi.fn(),
    onShortcut: vi.fn(),
    onFontSizeChange: vi.fn(),
    onMetadataChange: vi.fn(),
    onTerminalError: vi.fn(),
    windowsBuildNumber: null,
    contextMenuTriggerProps: {},
    actionsDisabled: false,
    zoomedPaneId: null as string | null,
    ask: vi.fn().mockResolvedValue({ confirmed: true, value: "", remember: false }),
    onConfirmLinkHost: vi.fn().mockResolvedValue(true),
    ...overrides,
  };
}

beforeEach(() => {
  mountSpy.mockClear();
  unmountSpy.mockClear();
  termPropsSpy.mockClear();
});

afterEach(() => {
  cleanup();
});

describe("PaneCanvas — 탭 전환 시 팬 재마운트 방지", () => {
  it("새 세션용 initialCommand만 전달하고 저장된 명령은 재연결 때 다시 실행하지 않는다", () => {
    const fresh = { ...pane("fresh"), startCommand: "pnpm dev", initialCommand: "pnpm dev" };
    const resumed = { ...pane("resumed"), startCommand: "pnpm dev", initialCommand: undefined };
    const tabs = [tab("t1", ["fresh", "resumed"])];

    render(<PaneCanvas {...baseProps({ tabs, panes: [fresh, resumed], activeTabId: "t1" })} />);

    expect(termPropsSpy).toHaveBeenCalledWith("fresh", "pnpm dev");
    expect(termPropsSpy).toHaveBeenCalledWith("resumed", undefined);
  });

  it("모든 팬을 panes 배열 순서대로, 활성 탭 여부와 무관하게 항상 마운트한다", () => {
    const panes = [pane("p1"), pane("p2")];
    const tabs = [tab("t1", ["p1"]), tab("t2", ["p2"])];

    render(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t1" })} />);

    // 비활성 탭(t2)의 p2도 마운트는 돼 있어야 한다 (display:none일 뿐 언마운트가 아님).
    expect(mountSpy).toHaveBeenCalledTimes(2);
    expect(mountSpy).toHaveBeenCalledWith("p1");
    expect(mountSpy).toHaveBeenCalledWith("p2");
  });

  it("탭을 전환해도 비활성화된 팬은 언마운트되지 않고, 새로 마운트되지도 않는다", () => {
    const panes = [pane("p1"), pane("p2")];
    const tabs = [tab("t1", ["p1"]), tab("t2", ["p2"])];

    const { rerender } = render(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t1" })} />);
    expect(mountSpy).toHaveBeenCalledTimes(2);
    mountSpy.mockClear();

    // t1 -> t2로 탭 전환. 실제 App.tsx가 하는 것처럼 activeTabId만 바꾼다.
    rerender(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t2" })} />);

    expect(unmountSpy).not.toHaveBeenCalled();
    // 같은 컴포넌트 인스턴스가 유지된다면 effect가 재실행되지 않으므로 새 마운트 호출도 없어야 한다.
    expect(mountSpy).not.toHaveBeenCalled();
  });

  it("탭을 여러 번 왕복 전환해도 한 번도 언마운트되지 않는다", () => {
    const panes = [pane("p1"), pane("p2"), pane("p3")];
    const tabs = [tab("t1", ["p1"]), tab("t2", ["p2"]), tab("t3", ["p3"])];

    const { rerender } = render(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t1" })} />);
    mountSpy.mockClear();

    for (const activeTabId of ["t2", "t3", "t1", "t3", "t2"]) {
      rerender(<PaneCanvas {...baseProps({ tabs, panes, activeTabId })} />);
    }

    expect(unmountSpy).not.toHaveBeenCalled();
    expect(mountSpy).not.toHaveBeenCalled();
  });

  it("팬을 다른 탭으로 옮겨도(paneIds만 이동) 언마운트되지 않는다", () => {
    const panes = [pane("p1"), pane("p2")];
    // 처음엔 p1, p2 둘 다 t1 소속.
    const before = [tab("t1", ["p1", "p2"]), tab("t2", [])];

    const { rerender } = render(<PaneCanvas {...baseProps({ tabs: before, panes, activeTabId: "t1" })} />);
    expect(mountSpy).toHaveBeenCalledTimes(2);
    mountSpy.mockClear();

    // App.tsx의 movePaneToTab처럼 panes 배열 자체는 그대로 두고 tabs[].paneIds만 바꾼다.
    const after = [tab("t1", ["p1"]), tab("t2", ["p2"])];
    rerender(<PaneCanvas {...baseProps({ tabs: after, panes, activeTabId: "t1" })} />);

    expect(unmountSpy).not.toHaveBeenCalled();
    expect(mountSpy).not.toHaveBeenCalled();
  });

  it("비활성 탭의 팬은 display:none, 활성 탭 안 팬은 order로만 구분한다", () => {
    const panes = [pane("p1"), pane("p2")];
    const tabs = [tab("t1", ["p1"]), tab("t2", ["p2"])];

    const { getByTestId, rerender } = render(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t1" })} />);

    expect(getByTestId("pane-p1").getAttribute("data-active")).toBe("true");
    expect(getByTestId("pane-p1").style.display).not.toBe("none");
    expect(getByTestId("pane-p2").getAttribute("data-active")).toBe("false");
    expect(getByTestId("pane-p2").style.display).toBe("none");

    rerender(<PaneCanvas {...baseProps({ tabs, panes, activeTabId: "t2" })} />);

    expect(getByTestId("pane-p1").getAttribute("data-active")).toBe("false");
    expect(getByTestId("pane-p1").style.display).toBe("none");
    expect(getByTestId("pane-p2").getAttribute("data-active")).toBe("true");
    expect(getByTestId("pane-p2").style.display).not.toBe("none");
  });
});

describe("PaneCanvas — grid 레이아웃 가드 (#189 회귀)", () => {
  it("활성 탭에 팬이 없어도 gridTemplateRows/Columns가 repeat(0, …)이 아니다", () => {
    // grid 분기의 gridRows/gridCols는 activePaneIds.length에서 나온다. 0개면
    // `repeat(0, 1fr)`이 나올 수 있는데, 이는 무효 CSS라 이전 렌더의 값이 그대로
    // 남는다. cols/rows 분기엔 있던 Math.max(1, …) 가드가 grid 분기에 누락됐던
    // 버그(#189)의 회귀를 막는다.
    const tabs = [tab("t1", [])];
    const { container } = render(<PaneCanvas {...baseProps({ tabs, panes: [], activeTabId: "t1" })} />);

    const panesEl = container.querySelector(".panes") as HTMLElement;
    expect(panesEl.style.gridTemplateRows).not.toContain("repeat(0");
    expect(panesEl.style.gridTemplateColumns).not.toContain("repeat(0");
  });
});

describe("PaneCanvas — workspace 복원 placeholder", () => {
  it("실패한 팬을 원래 순서에 남기고 안전한 정보와 재시도를 제공한다", () => {
    const failed: Pane = {
      key: "pane-failed",
      sessionId: null,
      distro: "Ubuntu",
      cwd: "/mnt/e/projects/devbox",
      multiplexer: "zellij",
      requestedMultiplexer: "zellij",
      restoreStatus: "failed",
      restoreError: "터미널을 복원하지 못했습니다.",
    };
    const onRetryPane = vi.fn();
    render(<PaneCanvas {...baseProps({
      tabs: [tab("t1", ["pane-failed"])],
      panes: [failed],
      activeTabId: "t1",
      activePaneId: "pane-failed",
      onRetryPane,
    })} />);

    expect(screen.getByRole("group", { name: "Ubuntu 터미널 복원 실패" })).toHaveFocus();
    expect(screen.getByText("/mnt/e/projects/devbox")).toBeInTheDocument();
    expect(screen.getByText("zellij")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    expect(onRetryPane).toHaveBeenCalledWith("pane-failed");
  });
});

describe("PaneCanvas — 크기 조절과 확대", () => {
  const twoPaneTabs = [{ ...tab("t1", ["p1", "p2"]), layout: "cols" as const }];
  const twoPanes = [pane("p1"), pane("p2")];

  it("workspace에 저장된 분할 비율을 첫 렌더부터 적용한다", () => {
    render(<PaneCanvas {...baseProps({
      tabs: [{ ...twoPaneTabs[0], sizing: { columns: [0.65, 0.35], rows: [1] } }],
      panes: twoPanes,
      activeTabId: "t1",
    })} />);

    expect((document.querySelector(".panes") as HTMLElement).style.gridTemplateColumns)
      .toBe("650fr 350fr");
  });

  it("세로 분할에는 팬 사이마다 구분선을 하나씩 둔다", () => {
    render(<PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />);
    const dividers = screen.getAllByRole("separator");
    expect(dividers).toHaveLength(1);
    expect(dividers[0]).toHaveAttribute("aria-orientation", "vertical");
    expect(dividers[0]).toHaveAttribute("aria-valuenow", "50");
  });

  it("방향키로 구분선을 옮기면 트랙 비율이 바뀌고 Home으로 되돌린다", () => {
    render(<PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />);
    const divider = screen.getByRole("separator");
    const canvas = document.querySelector(".panes") as HTMLElement;
    const before = canvas.style.gridTemplateColumns;

    fireEvent.keyDown(divider, { key: "ArrowRight" });
    expect(canvas.style.gridTemplateColumns).not.toBe(before);
    expect(divider).toHaveAttribute("aria-valuenow", "52");

    fireEvent.keyDown(divider, { key: "Home" });
    expect(canvas.style.gridTemplateColumns).toBe(before);
  });

  it("팬을 추가하면 조절한 비율은 균등 분할로 되돌아간다", () => {
    const { rerender } = render(
      <PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />,
    );
    fireEvent.keyDown(screen.getByRole("separator"), { key: "ArrowRight" });
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toBe("520fr 480fr");

    rerender(
      <PaneCanvas
        {...baseProps({
          tabs: [{ ...tab("t1", ["p1", "p2", "p3"]), layout: "cols" as const }],
          panes: [...twoPanes, pane("p3")],
          activeTabId: "t1",
        })}
      />,
    );
    expect(canvas.style.gridTemplateColumns).toBe("333fr 333fr 333fr");
  });

  it("팬을 추가했다 제거해도 이전 구성의 조절 비율이 되살아나지 않는다", () => {
    const { rerender } = render(
      <PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />,
    );
    fireEvent.keyDown(screen.getByRole("separator"), { key: "ArrowRight" });
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toBe("520fr 480fr");

    rerender(
      <PaneCanvas
        {...baseProps({
          tabs: [{ ...tab("t1", ["p1", "p2", "p3"]), layout: "cols" as const }],
          panes: [...twoPanes, pane("p3")],
          activeTabId: "t1",
        })}
      />,
    );
    expect(canvas.style.gridTemplateColumns).toBe("333fr 333fr 333fr");

    rerender(<PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />);
    expect(canvas.style.gridTemplateColumns).toBe("500fr 500fr");
  });

  it("레이아웃을 바꿨다 돌아와도 이전 레이아웃의 조절 비율이 되살아나지 않는다", () => {
    const { rerender } = render(
      <PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />,
    );
    fireEvent.keyDown(screen.getByRole("separator"), { key: "ArrowRight" });
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toBe("520fr 480fr");

    rerender(
      <PaneCanvas
        {...baseProps({
          tabs: [{ ...tab("t1", ["p1", "p2"]), layout: "rows" as const }],
          panes: twoPanes,
          activeTabId: "t1",
        })}
      />,
    );
    expect(canvas.style.gridTemplateRows).toBe("500fr 500fr");

    rerender(<PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />);
    expect(canvas.style.gridTemplateColumns).toBe("500fr 500fr");
  });

  it("가로 분할 구분선은 위아래 방향으로만 움직인다", () => {
    render(
      <PaneCanvas
        {...baseProps({
          tabs: [{ ...tab("t1", ["p1", "p2"]), layout: "rows" as const }],
          panes: twoPanes,
          activeTabId: "t1",
        })}
      />,
    );
    const divider = screen.getByRole("separator");
    expect(divider).toHaveAttribute("aria-orientation", "horizontal");
    const canvas = document.querySelector(".panes") as HTMLElement;

    fireEvent.keyDown(divider, { key: "ArrowRight" });
    expect(canvas.style.gridTemplateRows).toBe("500fr 500fr");
    fireEvent.keyDown(divider, { key: "ArrowDown" });
    expect(canvas.style.gridTemplateRows).toBe("520fr 480fr");
  });

  it("확대하면 그 팬만 보이고 구분선은 사라진다", () => {
    render(
      <PaneCanvas
        {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1", zoomedPaneId: "p2" })}
      />,
    );
    expect(screen.queryAllByRole("separator")).toHaveLength(0);
    expect(screen.getByTestId("pane-p2")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("pane-p1")).toHaveAttribute("data-active", "false");
  });

  it("확대를 켰다 꺼도 그 전의 팬 크기 비율은 유지한다", () => {
    const { rerender } = render(
      <PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />,
    );
    fireEvent.keyDown(screen.getByRole("separator"), { key: "ArrowRight" });
    const canvas = document.querySelector(".panes") as HTMLElement;
    expect(canvas.style.gridTemplateColumns).toBe("520fr 480fr");

    rerender(
      <PaneCanvas
        {...baseProps({
          tabs: twoPaneTabs,
          panes: twoPanes,
          activeTabId: "t1",
          zoomedPaneId: "p2",
        })}
      />,
    );
    expect(canvas.style.gridTemplateColumns).toBe("1000fr");

    rerender(<PaneCanvas {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1" })} />);
    expect(canvas.style.gridTemplateColumns).toBe("520fr 480fr");
  });

  it("확대한 팬이 활성 탭에 없으면 확대를 무시한다", () => {
    render(
      <PaneCanvas
        {...baseProps({ tabs: twoPaneTabs, panes: twoPanes, activeTabId: "t1", zoomedPaneId: "other" })}
      />,
    );
    expect(screen.getByTestId("pane-p1")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("pane-p2")).toHaveAttribute("data-active", "true");
  });
});
