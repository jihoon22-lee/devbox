import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import QuickOpen from "./QuickOpen";
import type { WorkspaceFile } from "../types";

const files: WorkspaceFile[] = [
  { path: "/workspace/README.md", relativePath: "README.md", size: 10 },
  {
    path: "/workspace/src/components/very/deep/very-long-file-name.ts",
    relativePath: "src/components/very/deep/very-long-file-name.ts",
    size: 2048,
  },
  { path: "/workspace/src/other.ts", relativePath: "src/other.ts", size: 10 },
];

afterEach(() => cleanup());

describe("QuickOpen", () => {
  it("explains the missing workspace instead of exposing stale file rows", () => {
    const rendered = render(
      <QuickOpen
        files={files}
        truncated={false}
        loading={false}
        workspaceFolder={null}
        onOpen={() => undefined}
        onClose={() => undefined}
      />,
    );

    expect(rendered.getByText("먼저 작업 폴더를 지정하세요.")).toBeTruthy();
    expect(rendered.queryByRole("option")).toBeNull();
  });

  it("renders matches as nested directory groups and preserves the full path as a title", () => {
    const rendered = render(
      <QuickOpen
        files={files}
        truncated={false}
        loading={false}
        workspaceFolder="C:\\work\\project"
        onOpen={() => undefined}
        onClose={() => undefined}
      />,
    );

    expect(rendered.getByRole("group", { name: "디렉터리 src" })).toBeTruthy();
    expect(rendered.getByRole("group", { name: "디렉터리 src/components/very/deep" })).toBeTruthy();
    const result = rendered.getByRole("option", { name: "src/components/very/deep/very-long-file-name.ts" });
    expect(result.getAttribute("title")).toBe("src/components/very/deep/very-long-file-name.ts");
    expect(result.querySelector(".quick-open-item-name")?.textContent).toBe("very-long-file-name.ts");
    expect(result.querySelector(".quick-open-item-path")?.textContent).toBe("src/components/very/deep/");
  });

  it("keeps the input as the keyboard focus target and opens the selected result", () => {
    const onOpen = vi.fn();
    const rendered = render(
      <QuickOpen
        files={files}
        truncated={false}
        loading={false}
        workspaceFolder="/workspace"
        onOpen={onOpen}
        onClose={() => undefined}
      />,
    );
    const input = rendered.getByRole("combobox", { name: "파일 검색" });
    expect(document.activeElement).toBe(input);

    fireEvent.change(input, { target: { value: "very-long" } });
    expect(input.getAttribute("aria-activedescendant")).toBe("quick-open-option-0");
    expect(
      rendered
        .getByRole("option", { name: "src/components/very/deep/very-long-file-name.ts" })
        .getAttribute("tabindex"),
    ).toBe("-1");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith("/workspace/src/components/very/deep/very-long-file-name.ts");
  });

  it("supports Home/End and Escape without requiring a pointer", () => {
    const onOpen = vi.fn();
    const onClose = vi.fn();
    const returnTarget = document.createElement("button");
    document.body.append(returnTarget);
    returnTarget.focus();
    const rendered = render(
      <QuickOpen
        files={files}
        truncated
        loading={false}
        workspaceFolder="/workspace"
        onOpen={onOpen}
        onClose={onClose}
      />,
    );
    const input = rendered.getByRole("combobox", { name: "파일 검색" });
    fireEvent.keyDown(input, { key: "Tab" });
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input.getAttribute("aria-activedescendant")).toBe("quick-open-option-2");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input.getAttribute("aria-activedescendant")).toBe("quick-open-option-0");
    fireEvent.keyDown(input, { key: "End" });
    const lastOption = rendered.container.querySelector('[aria-selected="true"]');
    expect(lastOption).toBeTruthy();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith("/workspace/src/components/very/deep/very-long-file-name.ts");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(rendered.getByText("폴더가 커서 일부만 색인했습니다")).toBeTruthy();
    rendered.unmount();
    expect(document.activeElement).toBe(returnTarget);
    returnTarget.remove();
  });
});
