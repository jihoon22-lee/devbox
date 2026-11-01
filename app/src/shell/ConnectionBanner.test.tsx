import { act, fireEvent, render, screen } from "@testing-library/react";
import { RpcClient, type Transport, type TransportState } from "../rpc/client";
import { jsonFrame } from "../rpc/frame";
import { RpcProvider } from "../rpc/RpcProvider";
import { ConnectionBanner } from "./ConnectionBanner";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

/** The native bridge: it stays open while the daemon behind it never answers. */
class Bridge implements Transport {
  frames = new Set<(frame: Uint8Array) => void>();
  states = new Set<(state: TransportState) => void>();
  connect = vi.fn(() => this.emit("open"));
  close = vi.fn(() => this.emit("closed"));
  reconnect = vi.fn(() => this.emit("closed"));
  send() {}
  onFrame(cb: (frame: Uint8Array) => void) {
    this.frames.add(cb);
    return () => {
      this.frames.delete(cb);
    };
  }
  onState(cb: (state: TransportState) => void) {
    this.states.add(cb);
    return () => {
      this.states.delete(cb);
    };
  }
  emit(state: TransportState) {
    for (const cb of this.states) cb(state);
  }
  welcome() {
    for (const cb of this.frames) cb(jsonFrame({ type: "welcome", protocol: 1, version: "1.0.0-dev", daemonId: "d" }));
  }
}
beforeEach(() => {
  vi.useFakeTimers();
  mocks.invoke.mockReset();
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});
afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});
function connect() {
  const bridge = new Bridge();
  const client = new RpcClient(bridge, { version: "1.0.0-dev", instance: "test", client: "test" });
  render(
    <RpcProvider client={client}>
      <ConnectionBanner />
    </RpcProvider>,
  );
  return bridge;
}
/** One attempt without a Welcome: the client gives up after 5 s, then the bridge opens again. */
function silent(bridge: Bridge, attempts = 1) {
  for (let attempt = 0; attempt < attempts; attempt++) {
    act(() => vi.advanceTimersByTime(5000));
    act(() => bridge.emit("open"));
  }
}
const retry = () => screen.queryByRole("button", { name: "다시 시도" });

test("a first connection that never gets a Welcome offers [다시 시도] after 5 failed attempts", async () => {
  const bridge = connect();
  silent(bridge, 4);
  expect(retry()).not.toBeInTheDocument();
  silent(bridge);
  expect(screen.getByRole("status")).toHaveTextContent(
    "WSL 쪽 devbox가 응답하지 않습니다. [다시 시도]를 누르면 WSL 쪽 devbox를 다시 시작합니다.",
  );
  let finish!: () => void;
  mocks.invoke.mockReturnValue(
    new Promise<void>((resolve) => {
      finish = resolve;
    }),
  );
  fireEvent.click(retry()!);
  expect(retry()).toBeDisabled();
  fireEvent.click(retry()!);
  expect(mocks.invoke).toHaveBeenCalledOnce();
  expect(mocks.invoke).toHaveBeenCalledWith("gate_retry", { distro: null });
  await act(async () => finish());
  // The restart is under way: the offer comes back only after 5 more failed attempts.
  expect(retry()).not.toBeInTheDocument();
  silent(bridge, 4);
  expect(retry()).not.toBeInTheDocument();
  silent(bridge);
  expect(retry()).toBeEnabled();
});

test("a Welcome resets the count, and the reconnecting copy never says 데몬", () => {
  const bridge = connect();
  silent(bridge, 5);
  expect(retry()).toBeInTheDocument();
  act(() => bridge.welcome());
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  // A connection lost after its Welcome is not a failed attempt.
  act(() => bridge.emit("closed"));
  expect(screen.getByRole("status")).toHaveTextContent(/^WSL 쪽 devbox에 다시 연결하고 있습니다 · 마지막 갱신 /);
  act(() => bridge.emit("open"));
  silent(bridge, 4);
  expect(retry()).not.toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("데몬");
  silent(bridge);
  expect(retry()).toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("데몬");
});

test("a refused retry says so and can be tried again", async () => {
  const bridge = connect();
  silent(bridge, 5);
  mocks.invoke.mockRejectedValueOnce({ message: "다른 설치·정리 작업이 진행 중입니다" });
  await act(async () => fireEvent.click(retry()!));
  expect(screen.getByRole("status")).toHaveTextContent("다시 시도하지 못했습니다. 잠시 후 다시 시도하세요.");
  expect(retry()).toBeEnabled();
});

test("outside the Windows app there is no native retry to offer", () => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  const bridge = connect();
  act(() => bridge.welcome());
  act(() => bridge.emit("closed"));
  act(() => bridge.emit("open"));
  silent(bridge, 6);
  expect(retry()).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("WSL 쪽 devbox에 다시 연결하고 있습니다");
});
