import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useToasts } from "../../ui";
import { AddProjectDialog } from "./AddProjectDialog";
import { useCurrentProject } from "./store";
import { ProjectWire, projects } from "./test-wire";

function Harness() {
  const [open, setOpen] = useState(true);
  return <AddProjectDialog open={open} onOpenChange={setOpen} />;
}
test("scan retry, candidate filtering, pending lock and success selection", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Harness />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  expect(screen.getByRole("status")).toHaveTextContent("검색하는 중");
  await act(async () => wire.fail("projects.scan"));
  await userEvent.click(await screen.findByRole("button", { name: "다시 시도" }));
  await act(async () =>
    wire.reply("projects.scan", {
      candidates: [
        { path: "/fixture/devbox", name: "devbox", registered: false, isGit: true },
        { path: "/fixture/registered", name: "registered", registered: true, isGit: false },
      ],
    }),
  );
  expect(screen.queryByText("registered")).not.toBeInTheDocument();
  const candidate = await screen.findByRole("button", { name: /devbox/ });
  await userEvent.dblClick(candidate);
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  expect(candidate).toBeDisabled();
  expect(screen.getByRole("textbox")).toBeDisabled();
  expect(screen.getByRole("button", { name: "추가 중…" })).toBeDisabled();
  await act(async () => wire.reply("projects.add", projects[0]));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(useCurrentProject.getState().id).toBe("a");
});
test("manual path is trimmed; typed add errors keep dialog open and allow retry", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Harness />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await act(async () => wire.reply("projects.scan", { candidates: [] }));
  await userEvent.type(screen.getByRole("textbox"), "  /fixture/new  ");
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  expect(wire.request("projects.add").params).toEqual({ path: "/fixture/new" });
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("projects.add").id,
      error: { code: "projects.not_directory", detail: { path: "/fixture/new" } },
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("폴더");
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(2));
});

test.each([false, true])(
  "lost add response retries its intent; edited input declares a new intent (%s)",
  async (edit) => {
    const wire = new ProjectWire();
    render(
      <RpcProvider client={wire.client}>
        <Harness />
      </RpcProvider>,
    );
    await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
    await act(async () => wire.reply("projects.scan", { candidates: [] }));
    const input = screen.getByRole("textbox");
    await userEvent.type(input, "/fixture/new");
    await userEvent.click(screen.getByRole("button", { name: "추가" }));
    await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
    const first = wire.request("projects.add");
    await act(async () => wire.close());
    await screen.findByRole("alert");
    if (edit) {
      await userEvent.clear(input);
      await userEvent.type(input, "/fixture/new");
    }
    await act(async () => wire.connect());
    await waitFor(() => expect(wire.client.state).toBe("ready"));
    expect(wire.requests("projects.add")).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "추가" }));
    await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(2));
    const retryId = wire.request("projects.add", 1).clientRequestId;
    if (edit) expect(retryId).not.toBe(first.clientRequestId);
    else expect(retryId).toBe(first.clientRequestId);
  },
);

test("registration displays Git execution warnings without blocking the added project", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Harness />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await act(async () => wire.reply("projects.scan", { candidates: [] }));
  await userEvent.type(screen.getByRole("textbox"), "/fixture/new");
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.add", {
      ...projects[0],
      gitWarnings: [{ key: "core.hookspath" }, { key: "filter.owned.clean" }],
    }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(useCurrentProject.getState().id).toBe("a");
  expect(useToasts.getState().items.at(-1)).toMatchObject({
    tone: "warn",
    title: "외부 명령을 실행할 수 있는 Git 설정이 있습니다",
  });
  expect(useToasts.getState().items.at(-1)?.body).toContain("core.hookspath");
});
