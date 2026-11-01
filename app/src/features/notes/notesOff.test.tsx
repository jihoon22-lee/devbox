import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { Backlinks } from "./Backlinks";
import { TagList } from "./TagList";
import { NotesWire } from "./test-wire";

afterEach(cleanup);

/** Each panel with the notes call it reads. */
const PANELS = [
  { name: "backlinks", method: "notes.backlinks", panel: <Backlinks path="a.md" onOpen={() => {}} /> },
  { name: "tags", method: "notes.tags", panel: <TagList /> },
] as const;

for (const { name, method, panel } of PANELS) {
  test(`${name} under notes turned off say how to turn them back on, with no retry that cannot succeed`, async () => {
    const wire = new NotesWire();
    wire.handlers[method] = () => {
      throw new RpcError("notes.disabled");
    };
    render(<RpcProvider client={wire.client}>{panel}</RpcProvider>);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("노트 설정을 읽지 못해 노트를 껐습니다.");
    expect(alert).toHaveTextContent("설정 → 앱 진단 → [다시 설정]을 눌러 주세요.");
    // The daemon answers every notes call with this error until it restarts: retrying only repeats it.
    expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
    expect(wire.requests(method)).toHaveLength(1);
  });

  test(`${name} that failed otherwise show the message of its code and still offer a retry`, async () => {
    const wire = new NotesWire();
    wire.handlers[method] = () => {
      throw new RpcError("notes.busy");
    };
    render(<RpcProvider client={wire.client}>{panel}</RpcProvider>);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("노트 링크를 고치는 중입니다.");
    expect(alert).toHaveTextContent("잠시 뒤 다시 시도해 주세요.");
    await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    await waitFor(() => expect(wire.requests(method)).toHaveLength(2));
  });
}
