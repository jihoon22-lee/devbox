import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { OpenApiDefinitions } from "./OpenApiDefinitions";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../transport", () => ({ componentInvoke: () => invoke }));
vi.mock("./lib/isTauri", () => ({ isTauri: () => true }));
const id = "a2345678-1234-4234-8234-123456789abc";
const request = { method: "GET", url: "https://fixture.test/users", headers: [], params: [], cookies: [], multipart: [], body_kind: "none", body: "", auth: null, timeout_ms: 30000, requiresSecretReview: true };
const definition = { schemaVersion: 1, id, name: "Fixture", openApiVersion: "3.1", operations: [{ label: "GET /users", method: "GET", requestTarget: "/users", mockStatus: 200, request }] };
const summaries = [{ id, name: "Fixture", operationCount: 1 }];
beforeEach(() => invoke.mockReset()); afterEach(cleanup);
it("opens a stored projection as a preview and applies only after explicit replacement", async () => {
  invoke.mockImplementation(async method => method === "list_openapi_definitions" ? summaries : definition);
  const onApply = vi.fn(); render(<OpenApiDefinitions revision={0} linkedIds={null} onSummaries={vi.fn()} onApply={onApply} disabled={false} />);
  fireEvent.click(screen.getByText(/보관한 OpenAPI 작업 \(/));
  fireEvent.click(await screen.findByRole("button", { name: "Fixture · 1개" }));
  const apply = await screen.findByRole("button", { name: "현재 요청 대신 적용" });
  expect(onApply).not.toHaveBeenCalled(); expect(invoke).toHaveBeenLastCalledWith("get_openapi_definition", { id });
  fireEvent.click(apply); expect(onApply).toHaveBeenCalledWith(expect.objectContaining({ method: "GET", url: request.url }));
  expect(invoke.mock.calls.some(([method]) => method === "send_request")).toBe(false);
});
it("requires explicit delete and removes the native-deleted record even when the list refresh fails", async () => {
  invoke.mockResolvedValueOnce(summaries).mockResolvedValueOnce(definition).mockResolvedValueOnce(null).mockRejectedValueOnce(new Error("list unavailable"));
  const onSummaries = vi.fn(); render(<OpenApiDefinitions revision={0} linkedIds={[id]} onSummaries={onSummaries} onApply={vi.fn()} disabled={false} />);
  fireEvent.click(screen.getByText(/보관한 OpenAPI 작업 \(/)); fireEvent.click(await screen.findByRole("button", { name: "Fixture · 1개" }));
  fireEvent.click(await screen.findByRole("button", { name: "보관한 작업 삭제…" })); expect(invoke).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: "보관한 작업 삭제 확인" }));
  await waitFor(() => expect(onSummaries).toHaveBeenLastCalledWith([]));
  expect((await screen.findByRole("alert")).textContent).toContain("보관 상태를 확인");
  expect(screen.queryByRole("button", { name: "Fixture · 1개" })).toBeNull();
});
it("does not apply a late result after unmount", async () => {
  let resolve!: (value: unknown) => void;
  invoke.mockResolvedValueOnce(summaries).mockImplementationOnce(() => new Promise(done => { resolve = done; }));
  const onApply = vi.fn(); const view = render(<OpenApiDefinitions revision={0} linkedIds={null} onSummaries={vi.fn()} onApply={onApply} disabled={false} />);
  fireEvent.click(screen.getByText(/보관한 OpenAPI 작업 \(/)); fireEvent.click(await screen.findByRole("button", { name: "Fixture · 1개" })); view.unmount();
  await act(async () => resolve(definition)); expect(onApply).not.toHaveBeenCalled();
});
