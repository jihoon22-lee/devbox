import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { FileTree } from "./FileTree";

vi.mock("./store", () => ({ useSource: { getState: () => ({ open: vi.fn() }) } }));
afterEach(cleanup);
test("file tree requests the selected project and opens a folder lazily", async () => {
  const wire = new NotesWire();
  wire.handlers["files.tree"] = (params) => ({
    entries:
      params.dir === ""
        ? [{ key: { root: "project:p", path: "src" }, name: "src", kind: "directory" }]
        : [{ key: { root: "project:p", path: "src/a.ts" }, name: "a.ts", kind: "file" }],
    nextCursor: null,
  });
  const action = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <FileTree projectId="p" showHidden={false} onAction={action} />
    </RpcProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "src" }));
  await screen.findByRole("button", { name: "a.ts" });
  expect(wire.requests("files.tree")).toEqual([
    { projectId: "p", dir: "", showHidden: false, cursor: null },
    { projectId: "p", dir: "src", showHidden: false, cursor: null },
  ]);
  fireEvent.contextMenu(screen.getByRole("button", { name: "a.ts" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "이름 바꾸기" }));
  await waitFor(() => expect(action).toHaveBeenCalledWith("rename", expect.objectContaining({ name: "a.ts" })));
});
