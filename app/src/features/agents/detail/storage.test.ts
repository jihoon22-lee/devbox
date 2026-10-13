import { useDrafts } from "./drafts";
import { quoteForPrompt } from "./quote";
import { useSeen } from "./seen";

beforeEach(() => {
  useDrafts.setState({ text: {} });
  useSeen.setState({ entries: {} });
});
afterEach(() => vi.restoreAllMocks());
test("draft hydration rejects malformed values; storage access failure retains editable memory", async () => {
  localStorage.setItem("devbox.agentDrafts", JSON.stringify({ state: { text: { good: "안녕", bad: 5 } }, version: 0 }));
  await useDrafts.persist.rehydrate();
  expect(useDrafts.getState().text).toEqual({ good: "안녕" });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("unavailable");
  });
  useDrafts.getState().set("good", "초안");
  useDrafts.getState().append("good", "인용");
  expect(useDrafts.getState().text.good).toBe("초안\n인용");
});
test("seen hydration accepts only paired keys and status/count signatures", async () => {
  localStorage.setItem(
    "devbox.agentSeen",
    JSON.stringify({
      state: { entries: { '["a","x"]': "modified:1:2", '["a","y"]': "random", wrong: "modified:0:0" } },
      version: 0,
    }),
  );
  await useSeen.persist.rehydrate();
  expect(useSeen.getState().entries).toEqual({ '["a","x"]': "modified:1:2" });
});
test("quotes use side/range and a fence longer than code content", () => {
  expect(quoteForPrompt("a.txt", { side: "new", from: 2, to: 3, text: "```\ncontent" })).toBe(
    "`a.txt` L2-3:\n````\n```\ncontent\n````\n",
  );
});
