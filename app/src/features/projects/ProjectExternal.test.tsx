import { invoke } from "@tauri-apps/api/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { ProjectWire, projects } from "./test-wire";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
test("native menu keyboard reaches each approved typed action with only its project path", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  try {
    const wire = new ProjectWire();
    render(
      <RpcProvider client={wire.client}>
        <ProjectSwitcher open onOpenChange={() => {}} />
      </RpcProvider>,
    );
    await waitFor(() => expect(wire.requests()).toHaveLength(1));
    await act(async () => wire.reply("projects.list", { rev: "1", items: [projects[0]] }));
    const input = screen.getByRole("combobox");
    await screen.findByRole("button", { name: "devbox 외부 앱으로 열기" });
    fireEvent.keyDown(input, { key: "ArrowRight" });
    fireEvent.keyDown(input, { key: "ArrowRight" });
    fireEvent.keyDown(input, { key: "ArrowRight" });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByRole("menuitem", { name: "VS Code로 열기" });
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(input).toHaveFocus());
    fireEvent.keyDown(input, { key: "Enter" });
    await userEvent.click(await screen.findByRole("menuitem", { name: "VS Code로 열기" }));
    expect(invoke).toHaveBeenLastCalledWith("open_external", { kind: "vscode", path: "/fixture/devbox" });
    await waitFor(() => expect(input).toHaveFocus());
    for (const [label, kind] of [
      ["탐색기로 열기", "explorer"],
      ["Windows Terminal로 열기", "terminal"],
    ]) {
      await userEvent.click(screen.getByRole("button", { name: "devbox 외부 앱으로 열기" }));
      await userEvent.click(await screen.findByRole("menuitem", { name: label }));
      expect(invoke).toHaveBeenLastCalledWith("open_external", { kind, path: "/fixture/devbox" });
      await waitFor(() => expect(input).toHaveFocus());
    }
  } finally {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  }
});

test.each([false, true])(
  "mouse external menu restores focus and late completion cannot focus a closed dialog (failure=%s)",
  async (failure) => {
    Object.assign(window, { __TAURI_INTERNALS__: {} });
    try {
      let resolve!: () => void;
      let reject!: (error: Error) => void;
      const pending = new Promise<void>((done, fail) => {
        resolve = done;
        reject = fail;
      });
      vi.mocked(invoke).mockImplementationOnce(() => pending as Promise<never>);
      const wire = new ProjectWire();
      const content = (open: boolean) => (
        <RpcProvider client={wire.client}>
          <input aria-label="다른 화면" />
          <ProjectSwitcher open={open} onOpenChange={() => {}} />
        </RpcProvider>
      );
      const view = render(content(true));
      await waitFor(() => expect(wire.requests()).toHaveLength(1));
      await act(async () => wire.reply("projects.list", { rev: "1", items: [projects[0]] }));
      const input = screen.getByRole("combobox");
      await userEvent.click(await screen.findByRole("button", { name: "devbox 외부 앱으로 열기" }));
      await userEvent.click(await screen.findByRole("menuitem", { name: "탐색기로 열기" }));
      await waitFor(() => expect(input).toHaveFocus());
      view.rerender(content(false));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
      const outside = screen.getByRole("textbox", { name: "다른 화면" });
      outside.focus();
      await act(async () => {
        if (failure) reject(new Error("owned external failure"));
        else resolve();
      });
      expect(outside).toHaveFocus();
    } finally {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    }
  },
);
