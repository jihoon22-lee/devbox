import { fireEvent, render, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import CloseReview from "./CloseReview";
it("keeps the review visible and reports failed flush without approving close", async () => {
  const save = vi.fn(async () => {
    throw new Error("복구 기록 실패");
  });
  const cancel = vi.fn(async () => {});
  const view = render(
    <CloseReview
      reasons={["Files 편집"]}
      filesDirty
      onSave={save}
      onDiscard={vi.fn()}
      onCancel={cancel}
      onReturn={vi.fn()}
    />,
  );
  fireEvent.click(view.getByRole("button", { name: "파일 저장 후 종료" }));
  await waitFor(() => expect(view.getByRole("alert").textContent).toContain("복구 기록 실패"));
  expect(view.getByRole("dialog")).toBeDefined();
  expect(cancel).not.toHaveBeenCalled();
});
