import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ActivityConsent } from "./Consent";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

const state = (consent: boolean, legacyKnowledge = false, paused = false) => ({ consent, paused, legacyKnowledge });
beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockReset().mockResolvedValue(() => {});
});
afterEach(cleanup);
const NOTICE = "v0.9.0 Knowledge의 활동 기록이 켜져 있다면 한쪽만 켜 두세요";

test("before consent only the collect button is offered", async () => {
  mocks.invoke.mockResolvedValueOnce(state(false));
  render(<ActivityConsent />);
  const collect = await screen.findByRole("button", { name: "활동 기록 켜기" });
  expect(screen.getAllByRole("button")).toEqual([collect]);
  expect(screen.queryByText(NOTICE)).not.toBeInTheDocument();
  mocks.invoke.mockResolvedValueOnce(state(true));
  await userEvent.click(collect);
  expect(mocks.invoke).toHaveBeenLastCalledWith("activity_set", { consent: true });
  expect(await screen.findByRole("button", { name: "일시중지" })).toBeInTheDocument();
  mocks.invoke.mockResolvedValueOnce(state(false));
  await userEvent.click(screen.getByRole("button", { name: "활동 기록 끄기" }));
  expect(mocks.invoke).toHaveBeenLastCalledWith("activity_set", { consent: false });
  await waitFor(() => expect(screen.getAllByRole("button")).toHaveLength(1));
});

test("a v0.9.0 Knowledge store shows the one-collector notice", async () => {
  mocks.invoke.mockResolvedValueOnce(state(false, true));
  render(<ActivityConsent />);
  expect(await screen.findByText(NOTICE)).toBeInTheDocument();
  expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["활동 기록 켜기"]);
});

test("pause and resume follow the native state, including tray changes", async () => {
  let receive: ((event: { payload: unknown }) => void) | undefined;
  mocks.listen.mockImplementation(async (_name: string, cb: (event: { payload: unknown }) => void) => {
    receive = cb;
    return () => {};
  });
  mocks.invoke.mockResolvedValueOnce(state(true));
  render(<ActivityConsent />);
  mocks.invoke.mockResolvedValueOnce(state(true, false, true));
  await userEvent.click(await screen.findByRole("button", { name: "일시중지" }));
  expect(mocks.invoke).toHaveBeenLastCalledWith("activity_set", { paused: true });
  expect(await screen.findByRole("button", { name: "다시 시작" })).toBeInTheDocument();
  receive?.({ payload: state(true) });
  expect(await screen.findByRole("button", { name: "일시중지" })).toBeInTheDocument();
});

test("a failed change keeps the previous state and says so", async () => {
  mocks.invoke.mockResolvedValueOnce(state(false));
  render(<ActivityConsent />);
  mocks.invoke.mockRejectedValueOnce({ message: "io" }).mockResolvedValueOnce(state(false));
  await userEvent.click(await screen.findByRole("button", { name: "활동 기록 켜기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("활동 기록 설정을 바꾸지 못했습니다");
  expect(screen.getByRole("button", { name: "활동 기록 켜기" })).toBeInTheDocument();
});

test("a withdrawal that stopped but could not be saved says it may come back", async () => {
  mocks.invoke.mockResolvedValueOnce(state(true));
  render(<ActivityConsent />);
  mocks.invoke.mockRejectedValueOnce({ message: "disk full" }).mockResolvedValueOnce(state(false));
  await userEvent.click(await screen.findByRole("button", { name: "활동 기록 끄기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("다시 켜질 수 있으니 다시 시도하세요");
  expect(mocks.invoke).toHaveBeenLastCalledWith("activity_state");
  expect(screen.getByRole("button", { name: "활동 기록 켜기" })).toBeInTheDocument();
});

test("a late initial native read cannot overwrite a newer tray event", async () => {
  let finish!: (value: unknown) => void;
  let receive!: (event: { payload: unknown }) => void;
  mocks.invoke.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  mocks.listen.mockImplementation(async (_name: string, cb: typeof receive) => {
    receive = cb;
    return () => {};
  });
  render(<ActivityConsent />);
  receive({ payload: state(true, false, true) });
  expect(await screen.findByRole("button", { name: "다시 시작" })).toBeVisible();
  finish(state(true, false, false));
  await waitFor(() => expect(screen.getByRole("button", { name: "다시 시작" })).toBeVisible());
});
