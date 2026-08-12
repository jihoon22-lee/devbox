import { cleanup, fireEvent, render } from "@testing-library/react";
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
      />,
    );
    fireEvent.keyDown(getByRole("tab"), { key: "Delete" });
    expect(onClose).toHaveBeenCalledWith("one");
  });
});
