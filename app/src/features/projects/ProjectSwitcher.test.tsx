import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectDataProvider } from "./ProjectData";
import { ProjectSwitcher, ProjectTitle } from "./ProjectSwitcher";
import { useCurrentProject } from "./store";
import { projects, ProjectWire } from "./test-wire";
function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <ProjectDataProvider>
      <ProjectTitle onOpen={() => setOpen(true)} />
      <ProjectSwitcher open={open} onOpenChange={setOpen} />
    </ProjectDataProvider>
  );
}
function Standalone() {
  const [open, setOpen] = useState(true);
  return <ProjectSwitcher open={open} onOpenChange={setOpen} />;
}
async function setup(standalone = false) {
  const wire = new ProjectWire();
  render(<RpcProvider client={wire.client}>{standalone ? <Standalone /> : <Harness />}</RpcProvider>);
  await waitFor(() => expect(wire.requests()).toHaveLength(1));
  return wire;
}
async function list(wire: ProjectWire) {
  await act(async () => wire.reply("projects.list", { rev: "1", items: projects }));
}
beforeEach(() => {
  useCurrentProject.getState().set(undefined);
});
test("shared title/switcher have one subscription; keyboard selection, IME, reset and focus restoration", async () => {
  const wire = await setup();
  await list(wire);
  const trigger = screen.getByRole("button", { name: "프로젝트 전환" });
  await userEvent.click(trigger);
  const input = screen.getByRole("combobox");
  await userEvent.type(input, "family");
  expect(screen.getAllByRole("row")).toHaveLength(1);
  expect(document.getElementById(input.getAttribute("aria-activedescendant")!)).toHaveTextContent("family");
  fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
  expect(useCurrentProject.getState().id).toBeUndefined();
  expect(fireEvent.keyDown(input, { key: "ArrowDown" })).toBe(false);
  await userEvent.keyboard("{Enter}");
  expect(useCurrentProject.getState().id).toBe("b");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(trigger).toHaveTextContent("family");
  await userEvent.click(trigger);
  expect(screen.getByRole("combobox")).toHaveValue("");
  expect(screen.getAllByRole("row")).toHaveLength(2);
  expect(wire.sent.filter((m) => m.type === "subscribe")).toHaveLength(1);
  const sub = wire.sent.find((m) => m.type === "subscribe")!;
  await act(async () => wire.deliver({ type: "event", subId: sub.id, rev: "2", payload: { rev: "2" } }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
});
test("removal defaults to cancel, retains selection until authoritative success and restores nested focus", async () => {
  const wire = await setup(true);
  await list(wire);
  act(() => useCurrentProject.getState().set("a"));
  const remove = await screen.findByRole("button", { name: "devbox 제거" });
  await userEvent.click(remove);
  expect(screen.getByRole("button", { name: "취소" })).toHaveFocus();
  await userEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(wire.requests("projects.remove")).toHaveLength(0);
  await waitFor(() => expect(screen.getByRole("combobox")).toHaveFocus());
  await userEvent.keyboard("{ArrowDown}{ArrowRight}{Enter}");
  expect(screen.getByRole("dialog", { name: "프로젝트 제거" })).toHaveTextContent("family");
  await userEvent.keyboard("{Escape}");
  await userEvent.click(remove);
  await userEvent.click(screen.getByRole("button", { name: "프로젝트 제거" }));
  await waitFor(() => expect(wire.requests("projects.remove")).toHaveLength(1));
  expect(wire.requests("projects.remove")[0].params).toEqual({ id: "a" });
  expect(useCurrentProject.getState().id).toBe("a");
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("dialog", { name: "프로젝트" })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("제거 중");
  expect(screen.getByRole("button", { name: "프로젝트 추가" })).toBeDisabled();
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "ArrowDown" });
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
  expect(useCurrentProject.getState().id).toBe("a");
  await act(async () => wire.reply("projects.remove", { rev: "2" }));
  await waitFor(() => expect(useCurrentProject.getState().id).toBeUndefined());
});
test("list loading/error/retry does not erase persisted selection; empty filtering clamps active option", async () => {
  const wire = await setup(true);
  act(() => useCurrentProject.getState().set("b"));
  expect(screen.getByRole("status")).toHaveTextContent("불러오는 중");
  await act(async () => wire.fail("projects.list"));
  await userEvent.click(await screen.findByRole("button", { name: "다시 시도" }));
  expect(useCurrentProject.getState().id).toBe("b");
  await list(wire);
  const input = screen.getByRole("combobox");
  await userEvent.type(input, "absent");
  fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(input).not.toHaveAttribute("aria-activedescendant");
  await userEvent.clear(input);
  expect(within(screen.getByRole("grid")).getAllByRole("row")[0]).toHaveAttribute("aria-selected", "true");
});

test("nested add cancellation restores focus and successful add survives a later list error", async () => {
  const wire = await setup(true);
  await list(wire);
  const trigger = screen.getByRole("button", { name: "프로젝트 추가" });
  await userEvent.click(trigger);
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await act(async () => wire.reply("projects.scan", { candidates: [] }));
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(trigger).toHaveFocus());
  await userEvent.click(trigger);
  await userEvent.type(screen.getByRole("textbox", { name: "폴더 경로" }), "/fixture/added");
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(wire.requests("projects.add")).toHaveLength(1));
  await userEvent.keyboard("{Escape}");
  expect(screen.getByRole("dialog", { name: "프로젝트 추가" })).toBeInTheDocument();
  await act(async () => wire.reply("projects.add", { ...projects[0], id: "new" }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  await act(async () => wire.fail("projects.list"));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "프로젝트 추가" })).not.toBeInTheDocument());
  expect(useCurrentProject.getState().id).toBe("new");
  expect(await screen.findByRole("button", { name: "다시 시도" })).toBeInTheDocument();
});

test("grid keyboard reaches the remove action without selecting the project", async () => {
  const wire = await setup(true);
  await list(wire);
  const input = screen.getByRole("combobox");
  await screen.findByRole("button", { name: "devbox 제거" });
  expect(input).toHaveAttribute("aria-haspopup", "grid");
  fireEvent.keyDown(input, { key: "ArrowDown" });
  fireEvent.keyDown(input, { key: "ArrowRight" });
  expect(document.getElementById(input.getAttribute("aria-activedescendant")!)).toHaveAttribute(
    "aria-label",
    "family 제거",
  );
  fireEvent.keyDown(input, { key: "Enter" });
  expect(screen.getByRole("dialog", { name: "프로젝트 제거" })).toHaveTextContent("family");
  expect(useCurrentProject.getState().id).toBeUndefined();
  await userEvent.keyboard("{Escape}");
  expect(wire.requests("projects.remove")).toHaveLength(0);
  await waitFor(() => expect(input).toHaveFocus());
});

test("removed projects cannot be selected during a delayed refresh or after failure and reopen", async () => {
  const wire = await setup();
  await list(wire);
  const trigger = screen.getByRole("button", { name: "프로젝트 전환" });
  await userEvent.click(trigger);
  act(() => useCurrentProject.getState().set("a"));
  await userEvent.click(await screen.findByRole("button", { name: "devbox 제거" }));
  await userEvent.click(screen.getByRole("button", { name: "프로젝트 제거" }));
  await waitFor(() => expect(wire.requests("projects.remove")).toHaveLength(1));
  await act(async () => wire.reply("projects.remove", { rev: "4" }));
  await waitFor(() => expect(wire.requests()).toHaveLength(2));
  expect(screen.queryByRole("button", { name: "devbox 제거" })).not.toBeInTheDocument();
  await userEvent.type(screen.getByRole("combobox"), "devbox");
  await userEvent.keyboard("{Enter}");
  expect(useCurrentProject.getState().id).toBeUndefined();
  await act(async () => wire.fail("projects.list"));
  await screen.findByRole("alert");
  await userEvent.keyboard("{Escape}");
  await userEvent.click(trigger);
  expect(screen.queryByRole("button", { name: "devbox 제거" })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(wire.requests()).toHaveLength(3));
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
  expect(useCurrentProject.getState().id).toBe("b");
  // A newer authoritative list still brings unrelated changes through.
  await act(async () => wire.reply("projects.list", { rev: "5", items: [{ ...projects[1], name: "updated" }] }));
  await waitFor(() => expect(trigger).toHaveTextContent("updated"));
});

test("project query diagnostics are visible and copyable with retry available", async () => {
  const user = userEvent.setup();
  const wire = await setup(true);
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.requests()[0].id,
      error: { code: "internal" },
      diagnosticId: "owned-project-diagnostic",
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("owned-project-diagnostic");
  await user.click(screen.getByRole("button", { name: "진단 복사" }));
  expect(copy).toHaveBeenCalledWith("devbox 진단 ID: owned-project-diagnostic");
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeInTheDocument();
});
