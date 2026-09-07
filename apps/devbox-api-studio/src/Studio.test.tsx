import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import Studio from "./Studio";

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn(() => { throw new Error("native command in browser"); }) }));
afterEach(() => { cleanup(); localStorage.clear(); });

it("keeps the actual request draft through protocol and webhook navigation without starting native work", async () => {
  render(<Studio/>);
  const url = await screen.findByPlaceholderText("https://api.example.com/users");
  fireEvent.change(url, { target: { value: "http://127.0.0.1:9000/draft-only" } });
  const nav = screen.getByRole("navigation", { name: "제품 화면" });
  fireEvent.click(within(nav).getByRole("button", { name: "프로토콜" }));
  await waitFor(() => expect(within(nav).getByRole("button", { name: "프로토콜" }).getAttribute("aria-current")).toBe("page"));
  fireEvent.click(within(nav).getByRole("button", { name: "웹훅 및 모의 서버" }));
  await screen.findByText("Webhook Lab");
  fireEvent.click(within(nav).getByRole("button", { name: /^요청$/ }));
  expect((await screen.findByPlaceholderText("https://api.example.com/users") as HTMLInputElement).value).toBe("http://127.0.0.1:9000/draft-only");
});
