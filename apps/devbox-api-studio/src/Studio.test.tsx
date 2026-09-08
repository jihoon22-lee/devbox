import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import Studio from "./Studio";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn(() => { throw new Error("native command in browser"); }) }));
afterEach(() => { cleanup(); localStorage.clear(); });

it("keeps the actual request draft through protocol and webhook navigation without starting native work", async () => {
  render(<Studio/>);
  await screen.findByRole("navigation", { name: "제품 화면" });
  // The async description must mount the feature before waiting for its imports.
  // Await the real lazy module graph, rather than making transformer speed
  // part of this draft-lifetime unit test. Runtime budgets are measured separately.
  // The case timeout also allows the real multi-route graph under CI contention.
  await act(async () => { await vi.dynamicImportSettled(); });
  const url = await screen.findByPlaceholderText("https://api.example.com/users");
  fireEvent.change(url, { target: { value: "http://127.0.0.1:9000/draft-only" } });
  const nav = screen.getByRole("navigation", { name: "제품 화면" });
  fireEvent.click(within(nav).getByRole("button", { name: "프로토콜" }));
  await waitFor(() => expect(within(nav).getByRole("button", { name: "프로토콜" }).getAttribute("aria-current")).toBe("page"));
  fireEvent.click(within(nav).getByRole("button", { name: "웹훅 및 모의 서버" }));
  await act(async () => { await vi.dynamicImportSettled(); });
  await screen.findByText("Webhook Lab");
  fireEvent.click(within(nav).getByRole("button", { name: /^요청$/ }));
  expect((await screen.findByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value).toBe("http://127.0.0.1:9000/draft-only");
}, 15_000);

it("previews collections and History independently of the live request draft", async () => {
  const request = { method: "GET", url: "https://fixture.test/saved", headers: [], params: [], cookies: [], multipart: [], body_kind: "none", body: "", auth: null, timeout_ms: 30000, requiresSecretReview: false };
  localStorage.setItem("apip-history-v2", JSON.stringify({ version: 2, history: [{ id: "history-fixture", saved_at: 1000, request, status: 200 }] }));
  localStorage.setItem("apip-collections-v2", JSON.stringify({ version: 2, collections: [{ id: "collection-fixture", saved_at: 1000, request, name: "fixture collection", folder: "", requiresSecretReview: false }] }));
  render(<Studio/>);
  const nav = await screen.findByRole("navigation", { name: "제품 화면" });
  await act(async () => { await vi.dynamicImportSettled(); });
  const url = await screen.findByPlaceholderText("https://api.example.com/users") as HTMLInputElement;
  fireEvent.change(url, { target: { value: "https://fixture.test/unsaved-draft" } });
  const collection = await screen.findByLabelText("컬렉션 항목: fixture collection");
  fireEvent.click(within(collection).getByRole("button", { name: "컬렉션 요청 미리보기: fixture collection" }));
  const dialog = await screen.findByRole("dialog", { name: "저장된 요청 미리보기" });
  expect(url.value).toBe("https://fixture.test/unsaved-draft");
  fireEvent.click(within(dialog).getByRole("button", { name: "닫기" }));
  fireEvent.click(within(nav).getByRole("button", { name: "기록 및 콘솔" }));
  await act(async () => { await vi.dynamicImportSettled(); });
  const history = await screen.findByRole("region", { name: "HTTP 요청 기록" });
  fireEvent.click(within(history).getByRole("button", { name: /fixture.test\/saved/ }));
  expect(screen.getByRole("heading", { name: "gRPC 실행 요약" })).toBeTruthy();
  fireEvent.click(within(nav).getByRole("button", { name: /^요청$/ }));
  expect((await screen.findByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value).toBe("https://fixture.test/unsaved-draft");
  fireEvent.click(within(nav).getByRole("button", { name: "기록 및 콘솔" }));
  await act(async () => { await vi.dynamicImportSettled(); });
  fireEvent.click(within(screen.getByRole("region", { name: "HTTP 요청 기록" })).getByRole("button", { name: /fixture.test\/saved/ }));
  fireEvent.click(screen.getByRole("button", { name: "현재 초안 대신 열기" }));
  expect((await screen.findByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value).toBe("https://fixture.test/saved");
  expect(screen.getByRole("button", { name: "보내기" })).toBeTruthy();
}, 15_000);
