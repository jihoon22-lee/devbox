import { useState } from "react";
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { KeyValueEditor } from "./KeyValueEditor";
afterEach(cleanup);
function Editor() {
  const [rows, setRows] = useState([
    { key: "q", value: "one" },
    { key: "next", value: "two" },
  ]);
  return <KeyValueEditor rows={rows} onChange={setRows} namePlaceholder="쿼리 이름" />;
}
it("names rows and restores focus after adding and deleting", () => {
  render(<Editor />);
  expect(screen.getByRole("textbox", { name: "쿼리 1 이름" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "쿼리 q 삭제" }));
  expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "쿼리 1 이름" }));
  fireEvent.click(screen.getByRole("button", { name: "+ 추가" }));
  expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "쿼리 2 이름" }));
});
