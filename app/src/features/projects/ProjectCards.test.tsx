import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { useToasts } from "../../ui";
import { ProjectDataProvider } from "./ProjectData";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { useCurrentProject } from "./store";
import { ProjectWire, projects } from "./test-wire";

test("cards show actual summary and launch once before selecting and navigating", async () => {
  const wire = new ProjectWire();
  const navigate = vi.fn();
  const close = vi.fn();
  useCurrentProject.getState().set(undefined);
  render(
    <RpcProvider client={wire.client}>
      <ProjectSwitcher open onOpenChange={close} onLaunched={navigate} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: [projects[0]] }));
  await waitFor(() => expect(wire.requests("projects.summary")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.summary", {
      items: [
        { id: "a", branch: "main", dirty: true, ahead: 2, behind: 0, terminals: 3, agents: { open: 2, attention: 1 } },
      ],
    }),
  );
  await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(1));
  await act(async () =>
    wire.reply("projects.config", {
      config: { agent: {}, launch: { services: [], terminals: [{ name: "shell" }], agents: [] } },
    }),
  );
  expect(screen.getByText("main")).toBeInTheDocument();
  expect(screen.getByText("변경 있음")).toBeInTheDocument();
  expect(screen.getByText("앞섬 2")).toBeInTheDocument();
  expect(screen.getByText("에이전트 2 · 입력 대기 1")).toBeInTheDocument();
  expect(screen.getByText("터미널 3개")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "devbox 외부 앱으로 열기" })).not.toBeInTheDocument();
  const launch = await screen.findByRole("button", { name: "devbox 시작 구성 실행" });
  await userEvent.dblClick(launch);
  await waitFor(() => expect(wire.requests("projects.launch")).toHaveLength(1));
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
  expect(useCurrentProject.getState().id).toBeUndefined();
  await userEvent.keyboard("{Escape}");
  expect(close).not.toHaveBeenCalled();
  await act(async () =>
    wire.reply("projects.launch", { terminals: 1, agents: 0, terminalIds: ["launch-shell"], rev: "2" }),
  );
  await waitFor(() => expect(navigate).toHaveBeenCalledOnce());
  expect(useCurrentProject.getState().id).toBe("a");
  expect(useToasts.getState().items.at(-1)?.title).toBe("터미널 1개 · 에이전트 0개를 시작했습니다");
});

function Reopen() {
  const [open, setOpen] = useState(false);
  return (
    <ProjectDataProvider>
      <button type="button" onClick={() => setOpen(true)}>
        열기
      </button>
      <ProjectSwitcher open={open} onOpenChange={setOpen} />
    </ProjectDataProvider>
  );
}
test("summary is open-only, visible config is cached for 30 seconds and unknown Git state stays unknown", async () => {
  const wire = new ProjectWire();
  render(
    <RpcProvider client={wire.client}>
      <Reopen />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
  expect(wire.requests("projects.summary")).toHaveLength(0);
  expect(wire.requests("projects.config")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "열기" }));
  await waitFor(() => expect(wire.requests("projects.config")).toHaveLength(2));
  await act(async () => {
    wire.reply("projects.summary", {
      items: [{ id: "a", dirty: null, ahead: null, behind: null, terminals: 0, agents: { open: 0, attention: 0 } }],
    });
    for (const request of wire.requests("projects.config"))
      wire.deliver({
        type: "response",
        id: request.id,
        result: {
          config: { agent: {}, launch: { services: [], terminals: [], agents: [] } },
          ...(request.params && (request.params as { id: string }).id === "a"
            ? { error: { message: "owned malformed config" } }
            : {}),
        },
      });
  });
  expect(screen.queryByText("변경 없음")).not.toBeInTheDocument();
  expect(screen.queryByText(/앞섬/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /시작 구성 실행/ })).not.toBeInTheDocument();
  await userEvent.hover(await screen.findByLabelText("devbox 설정 오류"));
  expect(await screen.findByRole("tooltip")).toHaveTextContent("owned malformed config");
  fireEvent.pointerLeave(screen.getByLabelText("devbox 설정 오류"));
  await userEvent.type(screen.getByRole("combobox"), "family");
  await userEvent.clear(screen.getByRole("combobox"));
  expect(wire.requests("projects.config")).toHaveLength(2);
  await userEvent.keyboard("{Escape}");
  await userEvent.click(screen.getByRole("button", { name: "열기" }));
  await waitFor(() => expect(wire.requests("projects.summary")).toHaveLength(2));
  expect(wire.requests("projects.config")).toHaveLength(2);
});

test("favorite waits for authoritative revision, sorts first and preserves current selection", async () => {
  const wire = new ProjectWire();
  useCurrentProject.getState().set("a");
  render(
    <RpcProvider client={wire.client}>
      <ProjectSwitcher open onOpenChange={() => {}} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
  await userEvent.dblClick(await screen.findByRole("button", { name: "family 즐겨찾기" }));
  await waitFor(() => expect(wire.requests("projects.set_favorite")).toHaveLength(1));
  expect(wire.request("projects.set_favorite").params).toEqual({ id: "b", favorite: true });
  await act(async () => wire.reply("projects.set_favorite", { rev: "3" }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await act(async () =>
    wire.reply("projects.list", { rev: "2", items: [projects[0], { ...projects[1], favorite: true }] }),
  );
  await waitFor(() => expect(screen.getAllByRole("row")[0]).toHaveTextContent("family"));
  expect(useCurrentProject.getState().id).toBe("a");
  expect(screen.getByRole("button", { name: "family 즐겨찾기" })).toHaveAttribute("aria-pressed", "true");
});
