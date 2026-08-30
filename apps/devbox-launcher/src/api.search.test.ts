import { describe, expect, it } from "vitest";
import { CLIPBOARD_PREVIEW_ID, search } from "./api";

describe("Launcher browser search aliases", () => {
  it("keeps the stable clipboard alias after localizing the visible label", async () => {
    await expect(search("clipboard")).resolves.toEqual(expect.objectContaining({
      results: expect.arrayContaining([
        expect.objectContaining({
          id: CLIPBOARD_PREVIEW_ID,
          label: "클립보드 미리보기",
        }),
      ]),
    }));
  });
});
