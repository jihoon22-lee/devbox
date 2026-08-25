import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import TabBar from "./TabBar";
import type { Doc } from "../types";

function doc(id: string): Doc {
  return {
    id,
    path: `/workspace/${id}.ts`,
    text: id,
    encoding: { encodingKind: "utf8", bom: false },
    lineEnding: "lf",
    readOnly: false,
    size: id.length,
    mtimeNanos: "1",
    contentHash: "hash",
    lossy: false,
    durabilityWarning: null,
    dirty: false,
    revision: 0,
    cursor: 0,
    bookmarks: [],
  };
}

afterEach(() => cleanup());

describe("document tab semantics", () => {
  it("exposes a roving tab, tab-panel relationship, and arrow navigation", () => {
    const onActivate = vi.fn();
    const { getByRole } = render(
      <TabBar
        view={0}
        docs={[doc("one"), doc("two")]}
        docIds={["one", "two"]}
        activeDocId="one"
        onActivate={onActivate}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onContextAction={vi.fn()}
      />,
    );

    const tabs = getByRole("tablist");
    expect(tabs.getAttribute("aria-orientation")).toBe("horizontal");
    const first = getByRole("tab", { name: "one.ts" });
    const second = getByRole("tab", { name: "two.ts" });
    expect(first.getAttribute("aria-controls")).toBe("code-pad-editor-one");
    expect(first.getAttribute("aria-selected")).toBe("true");
    expect(first.getAttribute("tabindex")).toBe("0");
    expect(second.getAttribute("tabindex")).toBe("-1");

    fireEvent.keyDown(first, { key: "ArrowRight" });
    expect(onActivate).toHaveBeenCalledWith("two");
    expect(document.activeElement).toBe(second);
  });

  it("closes the focused tab with Delete", () => {
    const onClose = vi.fn();
    const { getByRole } = render(
      <TabBar
        view={0}
        docs={[doc("one")]}
        docIds={["one"]}
        activeDocId="one"
        onActivate={vi.fn()}
        onClose={onClose}
        onMove={vi.fn()}
        onContextAction={vi.fn()}
      />,
    );
    fireEvent.keyDown(getByRole("tab"), { key: "Delete" });
    expect(onClose).toHaveBeenCalledWith("one");
  });

  it("opens the exact target-aware menu and marks delete as dangerous", () => {
    const onActivate = vi.fn();
    const onContextAction = vi.fn();
    const rendered = render(
      <TabBar
        view={0}
        docs={[doc("one"), doc("two"), doc("three")]}
        docIds={["one", "two", "three"]}
        activeDocId="one"
        onActivate={onActivate}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onContextAction={onContextAction}
      />,
    );

    fireEvent.contextMenu(rendered.getByRole("tab", { name: "two.ts" }), { clientX: 40, clientY: 60 });
    expect(onActivate).toHaveBeenLastCalledWith("two");
    const menu = rendered.getByRole("menu", { name: "문서 탭 작업" });
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "닫기",
      "다른 탭 닫기",
      "오른쪽 탭 모두 닫기",
      "경로 복사",
      "탐색기에서 열기",
      "이름 변경",
      "삭제",
    ]);
    expect(within(menu).getByRole("menuitem", { name: "삭제" }).classList.contains("danger")).toBe(true);

    fireEvent.click(within(menu).getByRole("menuitem", { name: "오른쪽 탭 모두 닫기" }));
    expect(onContextAction).toHaveBeenCalledWith(0, "two", "close-right");
  });

  it("supports keyboard opening, restores tab focus, and ignores IME key events", async () => {
    const rendered = render(
      <TabBar
        view={0}
        docs={[doc("one")]}
        docIds={["one"]}
        activeDocId="one"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onContextAction={vi.fn()}
      />,
    );
    const tab = rendered.getByRole("tab", { name: "one.ts" });
    tab.focus();

    fireEvent.keyDown(tab, { key: "F10", shiftKey: true, isComposing: true });
    expect(rendered.queryByRole("menu")).toBeNull();

    fireEvent.keyDown(tab, { key: "F10", shiftKey: true });
    const menu = rendered.getByRole("menu", { name: "문서 탭 작업" });
    expect(within(menu).getByRole("menuitem", { name: "다른 탭 닫기" }).getAttribute("aria-disabled")).toBe("true");
    expect(within(menu).getByRole("menuitem", { name: "오른쪽 탭 모두 닫기" }).getAttribute("aria-disabled")).toBe("true");
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(tab));

    fireEvent.contextMenu(tab, { clientX: 10, clientY: 10 });
    rendered.rerender(
      <TabBar
        view={0}
        docs={[doc("one")]}
        docIds={["one"]}
        activeDocId="one"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onContextAction={vi.fn()}
        disabled
      />,
    );
    await waitFor(() => expect(rendered.queryByRole("menu")).toBeNull());
  });
});
