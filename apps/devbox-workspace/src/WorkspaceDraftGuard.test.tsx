import { useRef, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { describe as describeProduct } from "@devbox/product-shell/api";
import Source from "./Source";
import { componentCall } from "./native";
import { createContextTransition } from "./contextTransition";
vi.mock("./native", () => ({ componentCall: vi.fn() }));
afterEach(cleanup);
it("retains the actual Source commit draft when an Agent review tries to change context", async () => {
  const description = {
    ...(await describeProduct("workspace")),
    context: { projectId: "p", worktreeId: "w1", revision: 1, target: { kind: "windows" as const } },
  };
  const review = { executable: "C:\\Git\\git.exe", sources: [], executionKeys: [], environmentKeys: [] };
  let approved = false;
  vi.mocked(componentCall).mockImplementation(async (_description, _component, method) => {
    if (method === "approve_trust") approved = true;
    const status = { approved, hasApproval: approved, review };
    return method === "preview_trust" ? { previewId: "preview", status } : status;
  });
  const select = vi.fn(async () => {});
  function Fixture() {
    const dirty = useRef(false);
    const [error, setError] = useState("");
    const [guard] = useState(() =>
      createContextTransition(
        () => (dirty.current ? ["Source 초안"] : []),
        async () => {},
      ),
    );
    return (
      <>
        <Source
          description={description}
          root="C:\\fixture"
          onBusyChange={vi.fn()}
          onOpenFile={vi.fn()}
          onProposeWorktree={vi.fn()}
          editorPending={false}
          onDirtyChange={(value) => {
            dirty.current = value;
          }}
        />
        <button type="button" onClick={() => void guard(select).catch((cause) => setError(cause.message))}>
          Agent 작업 검토
        </button>
        <p>{error}</p>
      </>
    );
  }
  render(<Fixture />);
  fireEvent.click(await screen.findByRole("button", { name: "Git 실행 검토" }));
  fireEvent.click(await screen.findByRole("button", { name: "검토한 Git 실행 승인" }));
  const message = await screen.findByRole("textbox", { name: "커밋 메시지" });
  await waitFor(() => expect((message.closest("fieldset") as HTMLFieldSetElement).disabled).toBe(false));
  fireEvent.change(message, { target: { value: "retain original draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Agent 작업 검토" }));
  await screen.findByText(/Source 초안 내용을 정리/);
  expect(select).not.toHaveBeenCalled();
  expect((message as HTMLTextAreaElement).value).toBe("retain original draft");
});
