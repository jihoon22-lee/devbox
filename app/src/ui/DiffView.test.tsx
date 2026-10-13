import { fireEvent, render, screen } from "@testing-library/react";
import { DiffView } from "./DiffView";
import { parseUnifiedDiff, selectDiffRange } from "./diff";

const patch = (path: string) =>
  `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n`;
test("selection uses only numbered lines on its side; file changes reset shift anchor", () => {
  const select = vi.fn();
  const file = parseUnifiedDiff(patch("a"))[0]!;
  const view = render(<DiffView file={file} onSelectLines={select} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 후 1줄" }));
  fireEvent.click(screen.getByRole("button", { name: "변경 후 2줄" }), { shiftKey: true });
  expect(select).toHaveBeenLastCalledWith({ side: "new", from: 1, to: 2, text: "ONE\ntwo" });
  view.rerender(<DiffView file={parseUnifiedDiff(patch("a"))[0]!} onSelectLines={select} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 후 2줄" }), { shiftKey: true });
  expect(select).toHaveBeenLastCalledWith({ side: "new", from: 1, to: 2, text: "ONE\ntwo" });
  view.rerender(<DiffView file={parseUnifiedDiff(patch("b"))[0]!} onSelectLines={select} />);
  fireEvent.click(screen.getByRole("button", { name: "변경 후 2줄" }), { shiftKey: true });
  expect(select).toHaveBeenLastCalledWith({ side: "new", from: 2, to: 2, text: "two" });
  expect(
    selectDiffRange(
      [
        { kind: "add", new: 1, text: "a" },
        { kind: "add", new: 9, text: "b" },
      ],
      "new",
      1,
      9,
    ),
  ).toBeNull();
});
test("metadata-only patches use accurate text and visible limit is surfaced", () => {
  const mode = parseUnifiedDiff("diff --git a/script b/script\nold mode 100644\nnew mode 100755\n")[0]!;
  const view = render(<DiffView file={mode} />);
  expect(screen.getByText(/권한/)).toBeInTheDocument();
  view.rerender(<DiffView file={parseUnifiedDiff(patch("a"))[0]!} maxLines={2} />);
  expect(screen.getByRole("status")).toHaveTextContent("2줄까지만");
  expect(screen.queryByRole("button", { name: "변경 후 2줄" })).toBeNull();
});
