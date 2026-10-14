import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { waitFor } from "@testing-library/react";
import { invalidateIfNewer } from "../../../rpc/revisions";
import { freshReviewPatch } from "./reviewPatch";

test("normal topic cancellation cannot verify cached content while its replacement read is pending", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 0 } } });
  const key = ["agents.patch", { id: "a", path: "a.txt" }];
  const old = { patch: "old", truncated: false };
  const next = { patch: "new", truncated: false };
  qc.setQueryData(key, old);
  const replies: ((value: typeof old) => void)[] = [];
  const observer = new QueryObserver(qc, {
    queryKey: key,
    staleTime: Infinity,
    queryFn: () => new Promise<typeof old>((resolve) => replies.push(resolve)),
  });
  const off = observer.subscribe(() => {});
  let completed = false;
  const verified = freshReviewPatch(qc, "a", "a.txt", "snapshot").then((data) => {
    completed = true;
    return data;
  });
  void verified.catch(() => {});
  try {
    await waitFor(() => expect(replies).toHaveLength(1));
    const invalidated = invalidateIfNewer(qc, [key], null);
    await waitFor(() => expect(replies).toHaveLength(2));
    expect(completed).toBe(false);
    replies[1]!(next);
    await invalidated;
    await expect(verified).rejects.toMatchObject({ code: "connection_lost" });
    const retried = freshReviewPatch(qc, "a", "a.txt", "snapshot");
    await waitFor(() => expect(replies).toHaveLength(3));
    replies[2]!(next);
    expect(await retried).toEqual(next);
  } finally {
    for (const reply of replies) reply(next);
    off();
    qc.clear();
  }
});
