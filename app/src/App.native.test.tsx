import { act, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { version } from "../package.json";
import { App } from "./App";
import { decodeFrame, jsonFrame } from "./rpc/frame";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  Channel: class {
    onmessage = (_: ArrayBuffer) => {};
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "main" }) }));
vi.mock("./shell/Shell", () => ({ Shell: () => <p>연결된 앱 화면</p> }));
test("native StrictMode waits for Rust configuration and sends actual prod app Hello", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
  mocks.listen.mockImplementation(async (name, cb) => {
    const callbacks = listeners.get(name) ?? new Set();
    listeners.set(name, callbacks);
    callbacks.add(cb);
    return () => callbacks.delete(cb);
  });
  mocks.invoke.mockImplementation(async (cmd, args) => {
    if (cmd === "gate_status") return { state: "connecting", instance: "prod", revision: "1" };
    if (cmd === "rpc_attach") return { attachmentId: args.attachmentId, state: "open", revision: "1" };
  });
  const view = render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  try {
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("gate_status"));
    expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_attach")).toHaveLength(0);
    act(() =>
      listeners
        .get("devbox://gate")
        ?.forEach((cb) => cb({ payload: { state: "ready", instance: "prod", revision: "2" } })),
    );
    await screen.findByText("연결된 앱 화면");
    await waitFor(() => expect(mocks.invoke.mock.calls.filter((c) => c[0] === "rpc_send")).toHaveLength(1));
    const sent = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_send")?.[1];
    expect(JSON.parse(new TextDecoder().decode(decodeFrame(sent).body))).toEqual({
      type: "hello",
      version,
      protocol: 1,
      client: "app",
      instance: "prod",
    });
    const attach = mocks.invoke.mock.calls.find((c) => c[0] === "rpc_attach")?.[1];
    act(() =>
      attach.onFrame.onmessage(jsonFrame({ type: "welcome", version, protocol: 1, daemonId: "test-daemon" }).buffer),
    );
    act(() =>
      listeners
        .get("devbox://gate")
        ?.forEach((cb) => cb({ payload: { state: "bridgeFailed", instance: "prod", revision: "3" } })),
    );
    expect(screen.getByText("연결된 앱 화면")).toBeInTheDocument();
  } finally {
    view.unmount();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  }
  expect(listeners.get("devbox://gate")?.size).toBe(0);
  expect(listeners.get("devbox://transport")?.size).toBe(0);
});
