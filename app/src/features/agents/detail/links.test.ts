import { waitFor } from "@testing-library/react";
import { openExternal } from "../../../shell/external";
import { useToasts } from "../../../ui";
import { openAgentPr } from "./links";

vi.mock("../../../shell/external", () => ({
  openExternal: vi.fn().mockRejectedValue(new Error("fixture opener unavailable")),
}));
test("PR opener failure is visible and its rejection is handled", async () => {
  openAgentPr("https://github.com/example/fixture/pull/1");
  await waitFor(() =>
    expect(useToasts.getState().items.some((item) => item.title === "PR을 열지 못했습니다")).toBe(true),
  );
  expect(openExternal).toHaveBeenCalledWith("https://github.com/example/fixture/pull/1");
});
