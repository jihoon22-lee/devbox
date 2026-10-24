import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ChangesList } from "./ChangesList";
import { LogView } from "./LogView";
import { SourceDiff } from "./SourceDiff";

afterEach(cleanup);
test("review changes render historical metadata and selection without an RPC provider", () => {
  const select = vi.fn();
  render(
    <ChangesList
      mode="review"
      files={[{ path: "new.txt", oldPath: "old.txt", status: "renamed", added: 1, removed: 1 }]}
      selectedPath="new.txt"
      onSelectPath={select}
      isSeen={() => true}
    />,
  );
  const file = screen.getByRole("button", { name: "R old.txt → new.txt · +1 −1 ✓ 봤음" });
  expect(file).toHaveAttribute("aria-current", "true");
  fireEvent.click(file);
  expect(select).toHaveBeenCalledWith("new.txt");
  expect(screen.queryByRole("button", { name: /스테이징|커밋|삭제|버리기/ })).toBeNull();
});
test("review diff uses the supplied snapshot and exact old line citation with no mutation or live query", () => {
  const select = vi.fn();
  render(
    <SourceDiff
      mode="review"
      path="new.txt"
      snapshot={{
        patch: "diff --git a/old.txt b/new.txt\n--- a/old.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-old text\n+new text\n",
      }}
      onSelectLines={select}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "변경 전 1줄" }));
  expect(select).toHaveBeenCalledWith({ side: "old", from: 1, to: 1, text: "old text" });
  expect(screen.queryByRole("button", { name: /스테이징|커밋|삭제|버리기|다시 읽기/ })).toBeNull();
});
test("empty historical review data stays empty rather than falling back to source RPC", () => {
  render(
    <>
      <ChangesList mode="review" files={[]} onSelectPath={vi.fn()} />
      <SourceDiff mode="review" path="empty.txt" snapshot={{ patch: "" }} />
      <LogView mode="review" commits={[]} />
    </>,
  );
  expect(screen.getByText("표시할 텍스트 변경이 없습니다.")).toBeVisible();
  expect(screen.getByRole("region", { name: "커밋 기록" })).toBeVisible();
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});
