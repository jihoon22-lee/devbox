import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { KeyValueEditor, type KeyValueRow } from "./KeyValueEditor";

function Harness() {
  const [rows, setRows] = useState<KeyValueRow[]>([]);
  return <KeyValueEditor value={rows} onChange={setRows} />;
}
test("environment rows can be added, edited, and removed without shifting the surviving value", async () => {
  render(<Harness />);
  await userEvent.click(screen.getByRole("button", { name: "환경 변수 추가" }));
  await userEvent.type(screen.getByRole("textbox", { name: "환경 변수 이름 1" }), "PORT");
  await userEvent.type(screen.getByRole("textbox", { name: "환경 변수 값 1" }), "3000");
  await userEvent.click(screen.getByRole("button", { name: "환경 변수 추가" }));
  await userEvent.type(screen.getByRole("textbox", { name: "환경 변수 이름 2" }), "HOST");
  await userEvent.type(screen.getByRole("textbox", { name: "환경 변수 값 2" }), "localhost");
  await userEvent.click(screen.getByRole("button", { name: "환경 변수 삭제 1" }));
  expect(screen.getByRole("textbox", { name: "환경 변수 이름 1" })).toHaveValue("HOST");
  expect(screen.getByRole("textbox", { name: "환경 변수 값 1" })).toHaveValue("localhost");
});
