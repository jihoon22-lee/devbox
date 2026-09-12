import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { fixtureDescription } from "@devbox/product-shell/api";
import DevelopmentSessions from "./DevelopmentSessions";
import { componentCall } from "./native";

vi.mock("./native", () => ({ componentCall: vi.fn() }));
const call = vi.mocked(componentCall);
const context = { projectId: "project", worktreeId: "first", target: { kind: "windows" as const }, revision: 1 };
const description = { ...fixtureDescription("workspace"), context };
const session = { id: "10000000-0000-4000-8000-000000000001", context, revision: 2, planRevision: "a".repeat(64), phase: "review", issue: null };
const job = { id: "20000000-0000-4000-8000-000000000001", name: "개발 서버", kind: "service", targetKind: "windows", targetDistro: null, command: "synthetic-server", cwd: null, envConfigured: true };
beforeEach(() => {
  sessionStorage.clear();
  call.mockReset().mockImplementation(async (_description, _component, method) => {
    if (method === "development_candidates") return { jobs: [job], truncated: false };
    if (method === "development_sessions") return { sessions: [], intents: {} };
    if (method === "prepare_development_session") return { session, jobs: [job] };
    return {};
  });
});
afterEach(cleanup);

it("previews commands without starting them and keeps restore-only approval separate", async () => {
  render(<DevelopmentSessions description={description} registry={null} />);
  fireEvent.click(await screen.findByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "환경 확인·실행 검토" }));
  await screen.findByText("synthetic-server");
  expect(call.mock.calls.some(([, , method]) => method === "start_development_session")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "상태만 이어가기" }));
  await waitFor(() => expect(call.mock.calls.find(([, , method]) => method === "start_development_session")?.[3]).toEqual({ id: session.id, revision: 2, planRevision: session.planRevision, mode: "restoreOnly" }));
});

it("does not display a late plan from the previous worktree", async () => {
  let finish: ((value: unknown) => void) | undefined;
  call.mockImplementation(async (_description, _component, method) => {
    if (method === "development_candidates") return { jobs: [job], truncated: false };
    if (method === "development_sessions") return { sessions: [], intents: {} };
    if (method === "prepare_development_session") return new Promise(resolve => { finish = resolve; });
    return {};
  });
  const view = render(<DevelopmentSessions description={description} registry={null} />);
  await screen.findByRole("checkbox");
  fireEvent.click(screen.getByRole("button", { name: "환경 확인·실행 검토" }));
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(<DevelopmentSessions description={{ ...description, context: { ...context, worktreeId: "second" } }} registry={null} />);
  finish?.({ session, jobs: [job] });
  await waitFor(() => expect((screen.getByRole("button", { name: "환경 확인·실행 검토" }) as HTMLButtonElement).disabled).toBe(false));
  expect(screen.queryByText("synthetic-server")).toBeNull();
});
