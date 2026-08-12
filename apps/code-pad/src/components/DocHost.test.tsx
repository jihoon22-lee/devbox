import { cleanup, render } from "@testing-library/react";
import { useEffect, type CSSProperties } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DocHost from "./DocHost";
import type { Doc } from "../types";

const { mountSpy, unmountSpy } = vi.hoisted(() => ({
  mountSpy: vi.fn<(docId: string) => void>(),
  unmountSpy: vi.fn<(docId: string) => void>(),
}));

vi.mock("../editor/CodeEditor", () => ({
  default: (props: { docId: string; style?: CSSProperties }) => {
    useEffect(() => {
      mountSpy(props.docId);
      return () => unmountSpy(props.docId);
      // The mocked editor follows the real editor's docId lifetime boundary.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.docId]);
    return <div data-testid={`editor-${props.docId}`} style={props.style} />;
  },
}));

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
    dirty: false,
    revision: 0,
    cursor: 0,
    bookmarks: [],
  };
}

function props(overrides: Partial<Parameters<typeof DocHost>[0]> = {}) {
  return {
    docs: [doc("one"), doc("two"), doc("three")],
    views: [["one", "two"], ["three"]] as [string[], string[]],
    activeDocByView: ["one", "three"] as [string | null, string | null],
    split: true,
    fontSize: 13,
    onChange: vi.fn(),
    onFocusDoc: vi.fn(),
    ...overrides,
  };
}

beforeEach(() => {
  mountSpy.mockClear();
  unmountSpy.mockClear();
});

afterEach(() => cleanup());

describe("DocHost stable editor parent", () => {
  it("mounts every document in registry order, including inactive tabs", () => {
    const { container } = render(<DocHost {...props()} />);
    expect(mountSpy.mock.calls.map(([id]) => id)).toEqual(["one", "two", "three"]);
    expect(container.querySelector(".doc-host")?.getAttribute("data-doc-order")).toBe("one,two,three");
  });

  it("keeps editor instances mounted across tab switches and view moves", () => {
    const initial = props();
    const { container, rerender } = render(<DocHost {...initial} />);
    mountSpy.mockClear();

    rerender(
      <DocHost
        {...props({
          views: [["one"], ["three", "two"]],
          activeDocByView: ["one", "two"],
        })}
      />,
    );
    rerender(
      <DocHost
        {...props({
          views: [["two"], ["one", "three"]],
          activeDocByView: ["two", "three"],
        })}
      />,
    );

    expect(unmountSpy).not.toHaveBeenCalled();
    expect(mountSpy).not.toHaveBeenCalled();
    expect(container.querySelector(".doc-host")?.getAttribute("data-doc-order")).toBe("one,two,three");
  });
});
