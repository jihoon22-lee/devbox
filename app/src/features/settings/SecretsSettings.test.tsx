import { webcrypto } from "node:crypto";
import type { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider, useRpcSession } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { useSettingsUi } from "./open";
import { SecretsSettings } from "./SecretsSettings";
import { SettingsPage } from "./SettingsPage";

beforeEach(() => vi.stubGlobal("crypto", webcrypto));
afterEach(() => vi.unstubAllGlobals());

test("settings exposes the secrets settings tab", () => {
  useSettingsUi.getState().setTab("general");
  render(<SettingsPage />);
  expect(screen.getByRole("tab", { name: "비밀" })).toBeInTheDocument();
});

async function setup(key = { locked: false, keyMismatch: false }) {
  const wire = new ProjectWire();
  let cache!: QueryClient;
  function Probe() {
    cache = useRpcSession().qc;
    return null;
  }
  const view = render(
    <RpcProvider client={wire.client}>
      <Probe />
      <SecretsSettings />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("secrets.status")).toHaveLength(1));
  await act(async () => {
    wire.reply("secrets.status", key);
    wire.reply("secrets.list", [
      { name: "TOKEN", hasValue: true },
      { name: "MISSING", hasValue: false },
    ]);
  });
  await screen.findByText("TOKEN · 이 PC에 값 있음");
  return { wire, view, cache };
}
test("secret values clear on dispatch even on error and never enter lists or query caches", async () => {
  const { wire, cache } = await setup();
  expect(screen.getByText("MISSING · 이 PC에 값 없음")).toBeInTheDocument();
  expect(wire.requests("secrets.export_key")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "TOKEN 값 입력" }));
  const input = screen.getByLabelText("비밀 값");
  expect(input).toHaveAttribute("type", "password");
  fireEvent.change(input, { target: { value: "owned-secret-only" } });
  await userEvent.dblClick(screen.getByRole("button", { name: "값 저장" }));
  expect(input).toHaveValue("");
  expect(wire.requests("secrets.set")).toHaveLength(1);
  expect(wire.request("secrets.set").params).toEqual({ name: "TOKEN", value: "owned-secret-only" });
  expect(wire.request("secrets.set").clientRequestId).toEqual(expect.any(String));
  await act(async () => wire.fail("secrets.set"));
  expect(input).toHaveValue("");
  expect(within(screen.getByRole("list", { name: "비밀 이름 목록" })).queryByText("owned-secret-only")).toBeNull();
  expect(
    JSON.stringify(
      cache
        .getQueryCache()
        .getAll()
        .map((q) => q.state.data),
    ),
  ).not.toContain("owned-secret-only");
  expect(cache.getMutationCache().getAll()).toHaveLength(0);
});
test("explicit recovery export stays outside caches and closing or unmounting clears the view", async () => {
  const { wire, cache, view } = await setup();
  expect(wire.requests("secrets.export_key")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "복구 문자열 보기" }));
  expect(wire.requests("secrets.export_key")).toHaveLength(1);
  await act(async () => wire.reply("secrets.export_key", { recovery: "OWND-RECV" }));
  expect(await screen.findByText("OWND-RECV")).toBeInTheDocument();
  expect(
    JSON.stringify(
      cache
        .getQueryCache()
        .getAll()
        .map((q) => q.state.data),
    ),
  ).not.toContain("OWND-RECV");
  await userEvent.click(screen.getByRole("button", { name: "복구 문자열 닫기" }));
  expect(screen.queryByText("OWND-RECV")).toBeNull();
  view.unmount();
});
test("mismatch offers password recovery and confirmed reset while ordinary value writes are blocked", async () => {
  const { wire } = await setup({ locked: true, keyMismatch: true });
  expect(screen.getByText(/비밀 키가 다릅니다/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "값 저장" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "복구 문자열 보기" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "복구 문자열 입력" }));
  const recovery = screen.getByLabelText("복구 문자열");
  expect(recovery).toHaveAttribute("type", "password");
  fireEvent.change(recovery, { target: { value: "OWND-RECV" } });
  await userEvent.click(screen.getByRole("button", { name: "키 복구" }));
  expect(recovery).toHaveValue("");
  expect(wire.request("secrets.import_key").params).toEqual({ recovery: "OWND-RECV" });
  await act(async () => wire.fail("secrets.import_key"));
  expect(recovery).toHaveValue("");
  await userEvent.click(screen.getByRole("button", { name: "비밀 다시 입력" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/모든 비밀 값을 삭제/)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "취소" })).toHaveFocus();
  expect(wire.requests("secrets.reset")).toHaveLength(0);
  await userEvent.click(within(dialog).getByRole("button", { name: "취소" }));
  expect(wire.requests("secrets.reset")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "비밀 다시 입력" }));
  await userEvent.dblClick(screen.getByRole("button", { name: "모든 값 초기화" }));
  expect(wire.requests("secrets.reset")).toHaveLength(1);
  expect(wire.request("secrets.reset").params).toEqual({});
  await act(async () => wire.reply("secrets.reset", { rev: "2" }));
  expect(await screen.findByText(/초기화했습니다/)).toBeInTheDocument();
  expect(wire.requests("secrets.status").length).toBeGreaterThan(1);
});
test("delete always confirms the selected name and refreshes only after success", async () => {
  const { wire } = await setup();
  await userEvent.click(screen.getByRole("button", { name: "TOKEN 삭제" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/TOKEN의 이 PC 값을 삭제/)).toBeInTheDocument();
  expect(wire.requests("secrets.delete")).toHaveLength(0);
  await userEvent.dblClick(within(dialog).getByRole("button", { name: "삭제" }));
  expect(wire.requests("secrets.delete")).toHaveLength(1);
  expect(wire.request("secrets.delete").params).toEqual({ name: "TOKEN" });
  await act(async () => wire.reply("secrets.delete", { rev: "3" }));
  expect(await screen.findByText("이 PC의 값을 삭제했습니다")).toBeInTheDocument();
  expect(wire.requests("secrets.list").length).toBeGreaterThan(1);
});
test("locked state remains visible without exposing native key metadata", async () => {
  const { wire } = await setup({ locked: true, keyMismatch: false });
  expect(screen.getByText("키 상태: 잠김")).toBeInTheDocument();
  expect(wire.requests("secrets.key_status")).toHaveLength(0);
  expect(screen.queryByText(/키 ID|봉인 blob/)).toBeNull();
});
test("metadata query errors expose a retry without requesting recovery material", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <SecretsSettings />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("secrets.status")).toHaveLength(1));
  await act(async () => {
    wire.fail("secrets.status");
    wire.reply("secrets.list", []);
  });
  expect(await screen.findByRole("alert")).toHaveTextContent("비밀 정보를 불러오지 못했습니다");
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(wire.requests("secrets.status")).toHaveLength(2);
  expect(wire.requests("secrets.export_key")).toHaveLength(0);
  await act(async () => {
    wire.reply("secrets.status", { locked: false, keyMismatch: false });
    wire.reply("secrets.list", []);
  });
  expect(await screen.findByText("키 상태: 열림")).toBeInTheDocument();
});

test.each(["secrets.set", "secrets.import_key", "secrets.delete", "secrets.reset"] as const)(
  "%s reuses only the ambiguous request identity while keeping sensitive inputs empty",
  async (method) => {
    const { wire, cache } = await setup();
    const send = async () => {
      await waitFor(() => expect(screen.getByRole("button", { name: "복구 문자열 입력", hidden: true })).toBeEnabled());
      if (method === "secrets.set") {
        await userEvent.click(screen.getByRole("button", { name: "TOKEN 값 입력" }));
        fireEvent.change(screen.getByLabelText("비밀 값"), { target: { value: "owned-retry-value" } });
        await userEvent.click(screen.getByRole("button", { name: "값 저장" }));
        expect(screen.getByLabelText("비밀 값")).toHaveValue("");
      } else if (method === "secrets.import_key") {
        if (!screen.queryByLabelText("복구 문자열"))
          await userEvent.click(screen.getByRole("button", { name: "복구 문자열 입력" }));
        fireEvent.change(screen.getByLabelText("복구 문자열"), { target: { value: "OWND-RETR" } });
        await userEvent.click(screen.getByRole("button", { name: "키 복구" }));
        expect(screen.getByLabelText("복구 문자열")).toHaveValue("");
      } else {
        if (!screen.queryByRole("dialog"))
          await userEvent.click(
            screen.getByRole("button", { name: method === "secrets.reset" ? "비밀 다시 입력" : "TOKEN 삭제" }),
          );
        await userEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: method === "secrets.reset" ? "모든 값 초기화" : "삭제",
          }),
        );
      }
    };
    await send();
    await waitFor(() => expect(wire.requests(method)).toHaveLength(1));
    const first = wire.request(method).clientRequestId;
    await act(async () =>
      wire.deliver({ type: "response", id: wire.request(method, -1).id, error: { code: "connection_lost" } }),
    );
    await send();
    await waitFor(() => expect(wire.requests(method)).toHaveLength(2));
    expect(wire.request(method, -1).clientRequestId).toBe(first);
    await act(async () => wire.reply(method, { rev: "2" }));
    await send();
    await waitFor(() => expect(wire.requests(method)).toHaveLength(3));
    const afterSuccess = wire.request(method, -1).clientRequestId;
    expect(afterSuccess).not.toBe(first);
    await act(async () => wire.fail(method));
    await send();
    await waitFor(() => expect(wire.requests(method)).toHaveLength(4));
    expect(wire.request(method, -1).clientRequestId).not.toBe(afterSuccess);
    expect(cache.getMutationCache().getAll()).toHaveLength(0);
    expect(
      JSON.stringify(
        cache
          .getQueryCache()
          .getAll()
          .map((query) => query.state.data),
      ),
    ).not.toMatch(/owned-retry-value|OWND-RETR/);
    await act(async () => wire.reply(method, { rev: "3" }));
  },
);

test.each(["input", "daemon"] as const)("changed %s starts a new secret intent after response loss", async (change) => {
  const { wire } = await setup();
  await userEvent.click(screen.getByRole("button", { name: "TOKEN 값 입력" }));
  fireEvent.change(screen.getByLabelText("비밀 값"), { target: { value: "owned-first-value" } });
  await userEvent.click(screen.getByRole("button", { name: "값 저장" }));
  await waitFor(() => expect(wire.requests("secrets.set")).toHaveLength(1));
  const first = wire.request("secrets.set").clientRequestId;
  await act(async () =>
    wire.deliver({ type: "response", id: wire.request("secrets.set").id, error: { code: "connection_lost" } }),
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "값 저장" })).toBeEnabled());
  if (change === "daemon") wire.client.daemonId = "new-owned-daemon";
  fireEvent.change(screen.getByLabelText("비밀 값"), {
    target: { value: change === "input" ? "owned-second-value" : "owned-first-value" },
  });
  await userEvent.click(screen.getByRole("button", { name: "값 저장" }));
  await waitFor(() => expect(wire.requests("secrets.set")).toHaveLength(2));
  expect(wire.request("secrets.set", -1).clientRequestId).not.toBe(first);
  expect(screen.getByLabelText("비밀 값")).toHaveValue("");
  await act(async () => wire.reply("secrets.set", { rev: "2" }));
});
