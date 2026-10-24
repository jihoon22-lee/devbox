import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, expect, it, vi } from "vitest";
import type { ToolId } from "./catalog";
import { TransformsPage } from "./TransformsPage";

function Harness() {
  const [id, setId] = useState<ToolId>("hmac");
  return <TransformsPage toolId={id} onSelect={setId} />;
}
beforeEach(() => localStorage.clear());
it("shows all21 tools and drops sensitive input on switching tools", () => {
  const writes = vi.spyOn(Storage.prototype, "setItem");
  render(<Harness />);
  expect(screen.getByRole("combobox", { name: "변환 도구" }).querySelectorAll("option")).toHaveLength(21);
  fireEvent.change(screen.getByLabelText("HMAC 키"), { target: { value: "fixture-private-key" } });
  fireEvent.change(screen.getByRole("combobox", { name: "변환 도구" }), { target: { value: "json-format" } });
  fireEvent.change(screen.getByRole("combobox", { name: "변환 도구" }), { target: { value: "hmac" } });
  expect(screen.getByLabelText("HMAC 키")).toHaveValue("");
  expect(JSON.stringify(writes.mock.calls)).not.toContain("fixture-private-key");
  writes.mockRestore();
});
it("still works when recent storage is blocked", () => {
  const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  render(<TransformsPage toolId="json-format" onSelect={() => {}} />);
  expect(screen.getByRole("textbox", { name: "입력" })).toBeInTheDocument();
  get.mockRestore();
  set.mockRestore();
});
