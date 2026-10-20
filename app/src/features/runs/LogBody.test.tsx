import { fireEvent, render, screen } from "@testing-library/react";
import { LogBody } from "./LogBody";

const source = { kind: "run" as const, runId: "r" };
const line = { src: source, ts: "2026-10-20T00:00:00Z", stream: "stderr" as const, text: "" };

test("links file locations across ANSI spans while retaining their styles", () => {
  const open = vi.fn();
  render(<LogBody line={{ ...line, text: "\x1b[36msrc/app.ts\x1b[0m:12:3 error" }} onOpenLocation={open} />);
  const link = screen.getByRole("button", { name: "src/app.ts:12:3" });
  expect(screen.getByText("src/app.ts")).toHaveStyle({ color: "#11a8cd" });
  fireEvent.click(link);
  expect(open).toHaveBeenCalledWith({ path: "src/app.ts", line: 12, column: 3, source });
});

test.each<[string, string, number, number | undefined]>([
  ["src/한글.ts:42", "src/한글.ts", 42, undefined],
  ['"src/한 글.ts":12:3', "src/한 글.ts", 12, 3],
  ["'src/hello world.ts':7", "src/hello world.ts", 7, undefined],
  ["C:\\개발\\app.ts:8:2", "C:\\개발\\app.ts", 8, 2],
])("links Unicode and quoted filenames: %s", (text, path, row, column) => {
  const open = vi.fn();
  render(<LogBody line={{ ...line, text }} onOpenLocation={open} />);
  fireEvent.click(screen.getByRole("button", { name: text }));
  expect(open).toHaveBeenCalledWith({ path, line: row, ...(column === undefined ? {} : { column }), source });
});
