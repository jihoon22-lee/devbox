import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcError } from "../../rpc/client";
import { useRpcQuery } from "../../rpc/query";
import { useSettingsUi } from "./open";
import { SettingsPage } from "./SettingsPage";

vi.mock("../../rpc/query", () => ({ useRpcQuery: vi.fn() }));
test("settings loading and query errors are visible with diagnostic retry", async () => {
  useSettingsUi.getState().setTab("diagnostics");
  const refetch = vi.fn();
  vi.mocked(useRpcQuery).mockReturnValue({ isPending: true, isError: false, refetch } as unknown as ReturnType<
    typeof useRpcQuery
  >);
  const view = render(<SettingsPage />);
  expect(screen.getAllByRole("status")).toHaveLength(1);
  await userEvent.click(screen.getByRole("tab", { name: "정보" }));
  expect(screen.getByRole("status")).toBeInTheDocument();
  vi.mocked(useRpcQuery).mockReturnValue({
    isPending: false,
    isError: true,
    error: new RpcError("internal", undefined, "owned-diagnostic"),
    refetch,
  } as unknown as ReturnType<typeof useRpcQuery>);
  view.rerender(<SettingsPage />);
  expect(screen.getAllByText("owned-diagnostic")).toHaveLength(1);
  await userEvent.click(screen.getByRole("tab", { name: "앱 진단" }));
  expect(screen.getByText("owned-diagnostic")).toBeInTheDocument();
  await userEvent.click(screen.getAllByText("다시 시도")[0]!);
  expect(refetch).toHaveBeenCalledOnce();
});
