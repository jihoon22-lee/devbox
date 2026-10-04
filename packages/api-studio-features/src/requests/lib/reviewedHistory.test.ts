import { expect, it, vi } from "vitest";
import { reviewedHistoryRemoval } from "./reviewedHistory";
import { confirmAction } from "@devbox/product-shell/confirm";
import type { HistoryItem } from "../types";
import { emptyRequest } from "./importers";
vi.mock("@devbox/product-shell/confirm", () => ({ confirmAction: vi.fn() }));
it("reads the latest history after approval and preserves rows appended during review", async () => {
  let decide!: (accepted: boolean) => void;
  vi.mocked(confirmAction).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        decide = resolve;
      }),
  );
  const request: HistoryItem["request"] = {
    ...emptyRequest(),
    url: "https://synthetic.test",
    requiresSecretReview: false,
  };
  const original: HistoryItem = { id: "old", saved_at: 1, request };
  const completed: HistoryItem = { id: "new", saved_at: 2, request };
  let latest = [original];
  const result = reviewedHistoryRemoval(original, () => latest);
  latest = [completed, original];
  decide(true);
  expect((await result)?.history.map(({ id }) => id)).toEqual(["new"]);
});
it("does not return a deletion after owner unmount", async () => {
  vi.mocked(confirmAction).mockResolvedValueOnce(true);
  expect(
    await reviewedHistoryRemoval(
      {
        id: "old",
        saved_at: 1,
        request: { ...emptyRequest(), url: "https://synthetic.test", requiresSecretReview: false },
      },
      () => null,
    ),
  ).toBeNull();
});
