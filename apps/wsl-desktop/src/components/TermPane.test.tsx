import type { ComponentProps } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TermPane from "./TermPane";

/**
 * jsdom에는 캔버스 텍스트 측정이 없어 실제 xterm을 그릴 수 없다(PaneCanvas.test.tsx의
 * 주석 참고 — 그쪽은 TermPane 자체를 통째로 모킹해 우회한다). 여기서는 반대로 TermPane의
 * resize 바닥값 로직(§2.3)만 검증하려는 것이므로, xterm 표면(Terminal/FitAddon/
 * Unicode11Addon)을 rows/cols를 직접 조작할 수 있는 최소 스텁으로 대체하고 실제
 * TermPane을 렌더링한다.
 */
const { createdTerminals, fitSizes, observerState, FakeTerminal, FakeFitAddon, FakeUnicode11Addon } = vi.hoisted(() => {
  type TerminalOptions = {
    windowsPty?: { backend: string; buildNumber?: number };
  };
  type TerminalSize = { rows: number; cols: number };
  const createdTerminals: { rows: number; cols: number; options: TerminalOptions }[] = [];
  const fitSizes: TerminalSize[] = [];
  const observerState: { callback?: () => void } = {};

  class FakeTerminal {
    rows = 2;
    cols = 5;
    unicode = { activeVersion: "" };
    constructor(public options: TerminalOptions) {
      createdTerminals.push(this);
    }
    loadAddon() {}
    open() {}
    attachCustomKeyEventHandler() {}
    onData() {
      return { dispose: () => undefined };
    }
    write() {}
    focus() {}
    dispose() {}
  }

  class FakeFitAddon {
    fit() {
      const nextSize = fitSizes.shift();
      if (!nextSize) return;
      const term = createdTerminals[createdTerminals.length - 1];
      term.rows = nextSize.rows;
      term.cols = nextSize.cols;
    }
  }

  class FakeUnicode11Addon {}

  return { createdTerminals, fitSizes, observerState, FakeTerminal, FakeFitAddon, FakeUnicode11Addon };
});

vi.mock("@xterm/xterm", () => ({ Terminal: FakeTerminal }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: FakeFitAddon }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: FakeUnicode11Addon }));

const { mockResizeSession } = vi.hoisted(() => ({
  mockResizeSession: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../api", () => ({
  attachSession: vi.fn().mockResolvedValue(undefined),
  broadcast: vi.fn().mockResolvedValue(undefined),
  writeSession: vi.fn().mockResolvedValue(undefined),
  resizeSession: mockResizeSession,
}));

function baseProps(overrides: Partial<ComponentProps<typeof TermPane>> = {}): ComponentProps<typeof TermPane> {
  return {
    sessionId: "s1",
    title: "Ubuntu",
    active: true,
    isFocusedPane: false,
    broadcastOn: false,
    broadcastTargetIds: [],
    registerWrite: vi.fn(),
    unregisterWrite: vi.fn(),
    onClose: vi.fn(),
    onFocusPane: vi.fn(),
    onShortcut: vi.fn(),
    windowsBuildNumber: null,
    ...overrides,
  };
}

beforeEach(() => {
  createdTerminals.length = 0;
  fitSizes.length = 0;
  observerState.callback = undefined;
  mockResizeSession.mockClear();
  // TermPane 마운트는 항상 `new ResizeObserver(...)`를 호출한다. jsdom은 이를 구현하지
  // 않으므로(ReferenceError) 최소 스텁으로 대체한다. 이 스위트는 ResizeObserver의
  // 디바운스 경로가 아니라 mount 직접 호출 + active(rAF) 경로만으로 fitAndSendResize를
  // 구동하므로 observe/disconnect는 아무 일도 안 해도 된다.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        observerState.callback = callback;
      }

      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  // active: false → true 전환의 rAF 트리거를 동기적으로 즉시 실행되게 만들어, 테스트가
  // 실제 프레임을 기다리지 않고도 결정적으로 재확인(fitAndSendResize)을 유발할 수 있게 한다.
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    cb(0);
    return 0;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("TermPane — resize 바닥값 (§2.3)", () => {
  it("passes the Windows build number to xterm ConPTY options", () => {
    const props = { ...baseProps(), windowsBuildNumber: 22631 } as ComponentProps<typeof TermPane>;
    render(<TermPane {...props} />);

    expect(createdTerminals[0].options.windowsPty).toEqual({ backend: "conpty", buildNumber: 22631 });
  });

  it("바닥값(4행 20열) 미만이면 마운트 시 resizeSession을 호출하지 않는다", () => {
    // FakeTerminal 기본값은 2행 5열 — 둘 다 바닥값 미만이다.
    render(<TermPane {...baseProps()} />);
    expect(mockResizeSession).not.toHaveBeenCalled();
  });

  it("바닥값 미만을 거치는 동안 이전 유효 크기가 보존돼, 그 크기로 복귀하면 재전송하지 않는다", async () => {
    // registerWrite/unregisterWrite를 모든 렌더에서 같은 참조로 유지해야 한다 — 바뀌면
    // 마운트 effect(의존성 [sessionId, registerWrite, unregisterWrite])가 매번 다시
    // 돌아 Terminal이 재생성되며 아래 term 조작이 새 인스턴스를 놓치게 된다.
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    const props = (active: boolean) => baseProps({ registerWrite, unregisterWrite, active });

    const { rerender } = render(<TermPane {...props(false)} />);
    const term = createdTerminals[0];
    expect(mockResizeSession).not.toHaveBeenCalled();

    // 1) 유효 크기(24행 80열)로 자란다 → 전송되고, .then()에서 lastSizeRef에 커밋된다.
    term.rows = 24;
    term.cols = 80;
    rerender(<TermPane {...props(true)} />);
    await Promise.resolve();
    await Promise.resolve();
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenLastCalledWith("s1", 24, 80);

    // 2) 바닥값 미만(2행 5열)으로 줄어든다 → 전송하지 않는다. 아래 3)에서 이 구간 동안
    //    lastSizeRef가 (실수로) 갱신되지 않았음을 간접적으로 확인한다.
    rerender(<TermPane {...props(false)} />);
    term.rows = 2;
    term.cols = 5;
    rerender(<TermPane {...props(true)} />);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);

    // 3) 원래의 유효 크기(24행 80열)로 복귀한다. lastSizeRef가 2)에서 그대로였다면
    //    (24,80)은 이미 전송된 적이 있으므로 재전송하지 않아야 한다 — 만약 2)에서
    //    바닥값 미만 크기를 lastSizeRef에 잘못 커밋했다면 여기서 다시 호출돼 실패한다.
    rerender(<TermPane {...props(false)} />);
    term.rows = 24;
    term.cols = 80;
    rerender(<TermPane {...props(true)} />);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
  });

  it("retries the same dimensions when resizeSession rejects", async () => {
    vi.useFakeTimers();
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    render(<TermPane {...baseProps({ active: false, registerWrite, unregisterWrite })} />);
    const term = createdTerminals[0];
    term.rows = 24;
    term.cols = 80;
    mockResizeSession.mockRejectedValueOnce(new Error("resize rejected"));

    expect(observerState.callback).toBeDefined();
    observerState.callback!();
    vi.advanceTimersByTime(100);
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenNthCalledWith(1, "s1", 24, 80);

    // Let the rejection handler run without committing the rejected size.
    await Promise.resolve();
    await Promise.resolve();

    observerState.callback!();
    vi.advanceTimersByTime(100);
    expect(mockResizeSession).toHaveBeenCalledTimes(2);
    expect(mockResizeSession).toHaveBeenNthCalledWith(2, "s1", 24, 80);
  });

  it("cancels a pending ResizeObserver resize before the activation resize", async () => {
    vi.useFakeTimers();
    const registerWrite = vi.fn();
    const unregisterWrite = vi.fn();
    // Mount fit, activation fit, and a stale timer fit respectively. The third size
    // must never reach resizeSession when activation cancels the pending timer.
    fitSizes.push({ rows: 2, cols: 5 }, { rows: 24, cols: 80 }, { rows: 40, cols: 120 });
    const { rerender } = render(<TermPane {...baseProps({ active: false, registerWrite, unregisterWrite })} />);

    expect(observerState.callback).toBeDefined();
    observerState.callback!();
    const clearTimeoutSpy = vi.spyOn(window, "clearTimeout");
    const clearCallsBeforeActivation = clearTimeoutSpy.mock.invocationCallOrder.length;

    rerender(<TermPane {...baseProps({ active: true, registerWrite, unregisterWrite })} />);
    vi.runOnlyPendingTimers();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    expect(mockResizeSession).toHaveBeenNthCalledWith(1, "s1", 24, 80);
    const activationClearOrders = clearTimeoutSpy.mock.invocationCallOrder.slice(clearCallsBeforeActivation);
    expect(activationClearOrders).not.toHaveLength(0);
    expect(activationClearOrders[0]).toBeLessThan(mockResizeSession.mock.invocationCallOrder[0]);

    vi.advanceTimersByTime(100);
    await Promise.resolve();
    expect(mockResizeSession).toHaveBeenCalledTimes(1);
    clearTimeoutSpy.mockRestore();
  });
});
