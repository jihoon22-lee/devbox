import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./backend";
import { emptyDraft, row } from "./model/draft";
import { RunnerPanel } from "./RunnerPanel";
import type { RunEntry } from "./runner/runner";
import { Session } from "./runner/session";
import { FakeBackend, textResponse } from "./test-backend";

afterEach(cleanup);
const capture = { id: "c", enabled: true, variable: "token", source: "jsonPath" as const, target: "$.token" };
const entry = (id: string, extra: Partial<RunEntry> & { url?: string; headers?: RunEntry["draft"]["headers"] }) => ({
  id,
  name: id,
  reviewNeeded: extra.reviewNeeded ?? false,
  produces: extra.produces ?? [],
  draft: { ...emptyDraft(), url: extra.url ?? `https://x.test/${id}`, headers: extra.headers ?? [], ...extra.draft },
});

it("shows_producers_before_running_and_skip_reasons_after", async () => {
  const user = userEvent.setup();
  const backend = new FakeBackend();
  backend.reply = async () => textResponse('{"token":"t"}');
  const entries = [
    entry("login", { reviewNeeded: true, produces: ["token"], draft: { ...emptyDraft(), captures: [capture] } }),
    entry("me", { headers: [row("Authorization", "Bearer {{token}}")] }),
    entry("health", {}),
  ];
  render(
    <ApiBackendProvider backend={backend}>
      <RunnerPanel entries={entries} environment={[]} session={new Session()} />
    </ApiBackendProvider>,
  );
  const plan = screen.getByRole("list", { name: "실행 순서" });
  const me = within(plan).getByRole("listitem", { name: /me/ });
  expect(me).toHaveTextContent("token ← login");
  expect(within(plan).getByRole("listitem", { name: /login/ })).toHaveTextContent("만드는 값: token");
  // 실행 전에도 검토가 필요한 항목을 보인다(검토 권장 1).
  expect(within(plan).getByRole("listitem", { name: /login/ })).toHaveTextContent("비밀 검토 필요");
  expect(within(plan).getByRole("listitem", { name: /health/ })).not.toHaveTextContent("비밀 검토 필요");
  await user.click(screen.getByRole("button", { name: "실행" }));
  const results = await screen.findByRole("table", { name: "실행 결과" });
  await waitFor(() => expect(within(results).getAllByRole("row")).toHaveLength(4));
  expect(within(results).getByRole("row", { name: /login/ })).toHaveTextContent("비밀 검토가 필요합니다");
  expect(within(results).getByRole("row", { name: /me/ })).toHaveTextContent("출처 요청 실패: login");
  expect(within(results).getByRole("row", { name: /health/ })).toHaveTextContent("통과");
  expect(backend.send).toHaveBeenCalledOnce();
  expect(screen.getByText("통과 1 · 실패 0 · 오류 0 · 건너뜀 2")).toBeInTheDocument();
});

it("reorders_entries_before_running", async () => {
  const user = userEvent.setup();
  render(
    <ApiBackendProvider backend={new FakeBackend()}>
      <RunnerPanel entries={[entry("a", {}), entry("b", {})]} environment={[]} session={new Session()} />
    </ApiBackendProvider>,
  );
  await user.click(screen.getByRole("button", { name: "b 위로" }));
  const names = within(screen.getByRole("list", { name: "실행 순서" }))
    .getAllByRole("listitem")
    .map((item) => item.getAttribute("aria-label"));
  expect(names).toEqual(["1. b", "2. a"]);
});
