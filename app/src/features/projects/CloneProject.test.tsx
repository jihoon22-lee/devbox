import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { AddProjectDialog } from "./AddProjectDialog";
import { useCurrentProject } from "./store";
import { ProjectWire, projects } from "./test-wire";

test("clone validates URL, derives target and preserves override and pending close guard", async () => {
  const wire = new ProjectWire();
  const close = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <AddProjectDialog open onOpenChange={close} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await userEvent.click(screen.getByRole("tab", { name: "Git 주소로" }));
  const url = screen.getByRole("textbox", { name: "Git 주소" });
  await userEvent.type(url, "https://user:secret@example.com/repo.git");
  expect(screen.getByRole("button", { name: "가져오기" })).toBeDisabled();
  await userEvent.clear(url);
  await userEvent.type(url, "git@example.com:team/repo.git");
  const target = screen.getByRole("textbox", { name: "대상 폴더" });
  expect(target).toHaveValue("~/projects/repo");
  await userEvent.clear(target);
  await userEvent.type(target, "/fixture/custom");
  await userEvent.clear(url);
  await userEvent.type(url, "https://example.com/other.git");
  expect(target).toHaveValue("/fixture/custom");
  await userEvent.dblClick(screen.getByRole("button", { name: "가져오기" }));
  await waitFor(() => expect(wire.requests("projects.clone")).toHaveLength(1));
  expect(wire.request("projects.clone").params).toEqual({
    url: "https://example.com/other.git",
    dir: "/fixture/custom",
  });
  await userEvent.keyboard("{Escape}");
  await userEvent.click(screen.getByRole("tab", { name: "폴더에서" }));
  expect(screen.getByRole("tab", { name: "Git 주소로" })).toHaveAttribute("aria-selected", "true");
  expect(close).not.toHaveBeenCalled();
  await act(async () => wire.reply("projects.clone", { ...projects[0], id: "cloned" }));
  await waitFor(() => expect(close).toHaveBeenCalledWith(false));
  expect(useCurrentProject.getState().id).toBe("cloned");
});

test("clone typed authentication failure stays open, redacts credentials and retries a lost response intent", async () => {
  const wire = new ProjectWire();
  const close = vi.fn();
  render(
    <RpcProvider client={wire.client}>
      <AddProjectDialog open onOpenChange={close} />
    </RpcProvider>,
  );
  await waitFor(() => expect(wire.requests("projects.scan")).toHaveLength(1));
  await userEvent.click(screen.getByRole("tab", { name: "Git 주소로" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Git 주소" }), "https://example.com/repo.git");
  await userEvent.click(screen.getByRole("button", { name: "가져오기" }));
  await waitFor(() => expect(wire.requests("projects.clone")).toHaveLength(1));
  await act(async () =>
    wire.deliver({
      type: "response",
      id: wire.request("projects.clone").id,
      error: {
        code: "projects.clone_failed",
        detail: {
          path: "/fixture/repo",
          message: "Authentication failed: https://user:credential@example.com/repo?token=hidden",
        },
      },
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("Authentication failed");
  expect(screen.getByRole("alert")).not.toHaveTextContent("credential");
  expect(screen.getByRole("alert")).not.toHaveTextContent("hidden");
  expect(close).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "가져오기" }));
  await waitFor(() => expect(wire.requests("projects.clone")).toHaveLength(2));
  const first = wire.request("projects.clone", 1);
  await act(async () => wire.close());
  await screen.findByRole("alert");
  await act(async () => wire.connect());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  await userEvent.click(screen.getByRole("button", { name: "가져오기" }));
  await waitFor(() => expect(wire.requests("projects.clone")).toHaveLength(3));
  expect(wire.request("projects.clone", 2).clientRequestId).toBe(first.clientRequestId);
});
