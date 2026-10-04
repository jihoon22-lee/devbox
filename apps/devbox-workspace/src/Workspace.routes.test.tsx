import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ShellContentProps } from "@devbox/product-shell";
import { fixtureDescription } from "@devbox/product-shell/api";
import type { Registry } from "./RegistryGate";

const state = vi.hoisted(() => ({ route: "tasks", ready: true, selected: false, unmount: vi.fn() }));
const registry: Registry = {
  revision: 1,
  projects: [{ id: "p", name: "Fixture" }],
  worktrees: [
    {
      id: "w",
      projectId: "p",
      revision: 1,
      binding: { root: "C:/fixture", target: { kind: "windows" } },
      trustedDigest: null,
    },
  ],
};
vi.mock("@devbox/product-shell", () => ({
  ProductShell: ({ renderContent }: { renderContent: (props: ShellContentProps) => React.ReactNode }) => {
    const [route, navigate] = useState(state.route);
    const description = fixtureDescription("workspace");
    description.context = state.selected
      ? { projectId: "p", worktreeId: "w", revision: 1, target: { kind: "windows" } }
      : null;
    return (
      <>
        <nav>
          {["overview", "files", "tasks", "source"].map((value) => (
            <button key={value} onClick={() => navigate(value)}>
              {value}
            </button>
          ))}
        </nav>
        {renderContent({ description, route, navigate, refreshContext: async () => {} })}
      </>
    );
  },
}));
vi.mock("@devbox/product-shell/api", async (original) => ({
  ...(await original<typeof import("@devbox/product-shell/api")>()),
  nativeMode: true,
  productDataAvailable: () => true,
}));
vi.mock("./RegistryGate", () => ({
  default: ({
    onReady,
    onSnapshot,
    suggestedRoot,
    editing,
  }: {
    onReady: () => void;
    onSnapshot: (snapshot: Registry) => void;
    suggestedRoot?: { path: string };
    editing: boolean;
  }) => {
    useEffect(() => {
      onSnapshot(registry);
      if (state.ready) onReady();
      return state.unmount;
    }, [onReady, onSnapshot]);
    return (
      <section aria-label="프로젝트 관리">
        <input aria-label="등록 초안" defaultValue="" />
        <button onClick={onReady}>준비 재시도</button>
        <p>{editing ? "편집 보호" : "전환 가능"}</p>
        {suggestedRoot && <p>등록 검토: {suggestedRoot.path}</p>}
      </section>
    );
  },
}));
vi.mock("./Source", () => ({
  default: ({ onProposeWorktree }: { onProposeWorktree: (path: string) => void }) => (
    <button onClick={() => onProposeWorktree("C:/fixture-new")}>생성한 폴더 등록 검토</button>
  ),
}));
vi.mock("./NativeRuntimeRoutes", () => ({
  default: ({ route, onDirtyChange }: { route: string; onDirtyChange: (dirty: boolean) => void }) => (
    <section aria-label={route}>
      <button onClick={() => onDirtyChange(true)}>작업 편집</button>
    </section>
  ),
}));
vi.mock("@devbox/workspace-features/files", () => ({ default: () => <section aria-label="파일 편집기" /> }));
vi.mock("./IncomingFileReview", () => ({ default: () => null }));
vi.mock("./ContextStatus", () => ({ default: () => <p>현재 context</p> }));
vi.mock("./ProjectDefinitions", () => ({ default: () => null }));
vi.mock("./TerminalLogBridge", () => ({ default: () => null }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
import Workspace from "./Workspace";

beforeEach(() => {
  state.route = "tasks";
  state.ready = true;
  state.selected = false;
  state.unmount.mockClear();
});
afterEach(cleanup);
it("keeps setup retry visible until ready, then gives the current task its own screen", async () => {
  state.ready = false;
  render(<Workspace />);
  expect(await screen.findByRole("region", { name: "프로젝트 관리" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "준비 재시도" }));
  await screen.findByRole("region", { name: "tasks" });
  expect(screen.queryByRole("region", { name: "프로젝트 관리" })).toBeNull();
});
it("keeps registry drafts mounted across Files and Tasks and retains context transition protection", async () => {
  state.route = "overview";
  render(<Workspace />);
  fireEvent.change(await screen.findByRole("textbox", { name: "등록 초안" }), { target: { value: "keep draft" } });
  fireEvent.click(screen.getByRole("button", { name: "files" }));
  await screen.findByRole("region", { name: "파일 편집기" });
  expect(screen.queryByRole("region", { name: "프로젝트 관리" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "tasks" }));
  fireEvent.click(await screen.findByRole("button", { name: "작업 편집" }));
  expect(screen.queryByRole("region", { name: "프로젝트 관리" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "overview" }));
  expect((screen.getByRole("textbox", { name: "등록 초안" }) as HTMLInputElement).value).toBe("keep draft");
  expect(screen.getByText("편집 보호")).toBeTruthy();
  expect(state.unmount).not.toHaveBeenCalled();
});
it("shows Source worktree registration review in Overview without changing selected context", async () => {
  state.route = "source";
  state.selected = true;
  render(<Workspace />);
  fireEvent.click(await screen.findByRole("button", { name: "생성한 폴더 등록 검토" }));
  await waitFor(() => expect(screen.getByRole("region", { name: "프로젝트 관리" })).toBeTruthy());
  expect(screen.getByText("등록 검토: C:/fixture-new")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "생성한 폴더 등록 검토" })).toBeNull();
  expect(screen.getByText("현재 context")).toBeTruthy();
});
it("offers project selection from an unselected task without requiring an inline registration form", async () => {
  render(<Workspace />);
  fireEvent.click(await screen.findByRole("button", { name: "개요에서 프로젝트 선택" }));
  expect(screen.getByRole("region", { name: "프로젝트 관리" })).toBeTruthy();
});
