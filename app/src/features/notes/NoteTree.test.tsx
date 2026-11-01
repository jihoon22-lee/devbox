import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NoteTree } from "./NoteTree";
import { NotesWire } from "./test-wire";

afterEach(cleanup);

function show(wire: NotesWire) {
  return render(
    <RpcProvider client={wire.client}>
      <NoteTree />
    </RpcProvider>,
  );
}

test("a vault the daemon turned off says how to turn it back on, with no retry that cannot succeed", async () => {
  const wire = new NotesWire();
  wire.handlers["notes.tree"] = () => {
    throw new RpcError("notes.disabled");
  };
  show(wire);
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("노트 설정을 읽지 못해 노트를 껐습니다.");
  expect(alert).toHaveTextContent("설정 → 앱 진단 → [다시 설정]을 눌러 주세요.");
  // The daemon answers every notes call with this error until it restarts: retrying only repeats it.
  expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
  expect(wire.requests("notes.tree")).toHaveLength(1);
});

test("any other failure to read the tree shows the message of its code and still offers a retry", async () => {
  const wire = new NotesWire();
  wire.handlers["notes.tree"] = () => {
    throw new RpcError("notes.busy");
  };
  show(wire);
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("노트 링크를 고치는 중입니다.");
  expect(alert).toHaveTextContent("잠시 뒤 다시 시도해 주세요.");
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests("notes.tree")).toHaveLength(2));
});
