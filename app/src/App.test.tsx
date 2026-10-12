import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { App, pickTransport } from "./App";
import type { Transport } from "./rpc/client";

function transport(): Transport {
  return { connect: vi.fn(), close: vi.fn(), send: vi.fn(), onFrame: () => () => {}, onState: () => () => {} };
}
test("missing gateway shows Korean startup error with retry", async () => {
  const load = vi.fn().mockRejectedValueOnce(Error("sensitive-token")).mockResolvedValueOnce(transport());
  render(<App loadTransport={load} />);
  expect(screen.getByRole("status")).toHaveTextContent("연결을 준비하고 있습니다");
  await screen.findByRole("alert");
  expect(screen.queryByText("sensitive-token")).not.toBeInTheDocument();
  await userEvent.click(screen.getByText("다시 시도"));
  await screen.findByRole("navigation", { name: "섹션" });
  expect(load).toHaveBeenCalledTimes(2);
});
test("StrictMode disposes late transports and active session on unmount", async () => {
  const pending: Array<(value: Transport) => void> = [];
  const load = () => new Promise<Transport>((resolve) => pending.push(resolve));
  const view = render(
    <StrictMode>
      <App loadTransport={load} />
    </StrictMode>,
  );
  const abandoned = transport(),
    active = transport();
  await waitFor(() => expect(pending).toHaveLength(2));
  pending[0]?.(abandoned);
  pending[1]?.(active);
  await waitFor(() => expect(active.connect).toHaveBeenCalled());
  expect(abandoned.close).toHaveBeenCalledOnce();
  view.unmount();
  expect(active.close).toHaveBeenCalled();
});
test("gateway validation rejects non websocket URLs and accepts environment fallback", async () => {
  await expect(pickTransport("?gateway=https://localhost", "")).rejects.toThrow();
  await expect(pickTransport("", "ws://localhost:1451")).resolves.toBeDefined();
});

beforeEach(() => vi.spyOn(window, "scrollTo").mockImplementation(() => {}));
