import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { Preview } from "./Preview";
import { NotesWire } from "./test-wire";

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(async (id: string, source: string) => ({ svg: `<svg data-id="${id}"><text>${source}</text></svg>` })),
}));
vi.mock("mermaid", () => ({ default: mermaid }));
const external = vi.hoisted(() => ({ openExternal: vi.fn(async () => {}) }));
vi.mock("../../shell/external", () => external);

const HOSTILE = [
  "<p>hello</p>",
  '<img alt="remote" src="https://example.invalid/x.png">',
  '<img alt="protocol-relative" src="//example.invalid/y.png">',
  '<img alt="inline" src="data:image/png;base64,iVBORw0KGgo=">',
  '<picture><source srcset="https://example.invalid/z.png"></picture>',
  '<script>window.__owned = 1</script><iframe src="https://example.invalid"></iframe>',
  '<a href="/\\evil.example" onclick="window.__owned = 2">sneaky</a>',
  '<a href="//evil.example/x.md">sneaky note</a>',
  '<a href="https://example.invalid/page" target="_blank">web</a>',
  '<a href="sub/b.md#top">note b</a>',
  '<div class="mermaid-block" data-idx="0"></div>',
  '<div class="mermaid-block" data-idx="1" data-reason="budget"></div>',
].join("");

let wire: NotesWire;
const opened = vi.fn();
beforeEach(() => {
  opened.mockClear();
  wire = new NotesWire();
  wire.handlers["notes.render"] = () => ({ html: HOSTILE, mermaid: ["graph TD; A-->B", ""] });
  wire.handlers["notes.resolve"] = () => ({ paths: ["sub/b.md"], ambiguous: false });
});
afterEach(() => {
  cleanup();
  mermaid.initialize.mockClear();
  mermaid.render.mockClear();
  external.openExternal.mockClear();
});

async function shown() {
  const view = render(
    <RpcProvider client={wire.client}>
      <Preview path="sub/a.md" text="# a" onOpenNote={opened} />
    </RpcProvider>,
  );
  await screen.findByText("hello");
  return view.container.querySelector("[data-preview]") as HTMLElement;
}

test("the render request carries the note path so relative images resolve from its folder", async () => {
  await shown();
  expect(wire.requests("notes.render")).toEqual([{ path: "sub/a.md", text: "# a" }]);
});

test("remote images never reach the page; inline data images stay; scripts and handlers are dropped", async () => {
  const host = await shown();
  const sources = [...host.querySelectorAll("img")].map((img) => img.getAttribute("src"));
  expect(sources).toEqual(["data:image/png;base64,iVBORw0KGgo="]);
  expect(host.querySelectorAll('[data-reason="remote-blocked"]')).toHaveLength(2);
  expect(host.querySelector("source, picture, script, iframe")).toBeNull();
  expect(host.innerHTML).not.toContain("example.invalid/x.png");
  expect(host.innerHTML).not.toContain("onclick");
  expect((window as { __owned?: number }).__owned).toBeUndefined();
});

test("mermaid renders in strict mode, skips budget placeholders and replaces only its own block", async () => {
  const host = await shown();
  await waitFor(() => expect(host.querySelector("svg")).not.toBeNull());
  expect(mermaid.initialize).toHaveBeenCalledWith(
    expect.objectContaining({ startOnLoad: false, securityLevel: "strict" }),
  );
  expect(mermaid.render).toHaveBeenCalledTimes(1);
  expect(mermaid.render.mock.calls[0]?.[1]).toBe("graph TD; A-->B");
  expect(host.querySelector('[data-idx="0"] svg')).not.toBeNull();
  expect(host.querySelector('[data-idx="1"] svg')).toBeNull();
});

test("links cannot navigate the app: no href survives; web links open outside, note links open the note", async () => {
  const host = await shown();
  expect(host.querySelector("a[href], [target]")).toBeNull();
  const sneaky = screen.getByRole("link", { name: "sneaky" });
  expect(sneaky).toHaveAttribute("data-href", "/\\evil.example");
  expect(fireEvent.click(sneaky)).toBe(false);
  fireEvent.keyDown(sneaky, { key: "Enter" });
  fireEvent.click(screen.getByRole("link", { name: "sneaky note" }));
  expect(external.openExternal).not.toHaveBeenCalled();
  expect(wire.requests("notes.resolve")).toEqual([]);
  fireEvent.keyDown(screen.getByRole("link", { name: "web" }), { key: "Enter" });
  expect(external.openExternal).toHaveBeenCalledWith("https://example.invalid/page");
  expect(fireEvent.click(screen.getByRole("link", { name: "note b" }))).toBe(false);
  await waitFor(() => expect(opened).toHaveBeenCalledWith("sub/b.md"));
  expect(wire.requests("notes.resolve")).toEqual([{ target: "sub/b.md", kind: "md", from: "sub/a.md" }]);
});

test.each([
  'sequenceDiagram\nparticipant A\nproperties A: {"icon":"tracker.png"}\nA->>A: hi',
  "architecture-beta\nservice a(internet)[a]",
  'flowchart TD\nA@{ img: "https://example.invalid/x" }',
  '%%{init: {"htmlLabels":true}}%%\nflowchart TD\nA["<img src=https://example.invalid/x>"]',
  '---\nconfig:\n  themeCSS: "@import url(https://example.invalid/x)"\n---\nflowchart TD; A-->B',
  'flowchart TD\nA["&lt;img src=https://example.invalid/x&gt;"]',
  'flowchart TD\nA["#60;img src=https://example.invalid/x#62;"]',
  "flowchart TD\nclassDef image fill:url(https://example.invalid/x)",
])("resource-capable Mermaid input is rejected before rendering: %s", async (source) => {
  wire.handlers["notes.render"] = () => ({ html: '<div class="mermaid-block" data-idx="0"></div>', mermaid: [source] });
  render(
    <RpcProvider client={wire.client}>
      <Preview path="a.md" text="diagram" onOpenNote={opened} />
    </RpcProvider>,
  );
  await screen.findByText("외부 리소스 차단을 위해 이 다이어그램 종류나 문법은 표시하지 않습니다.");
  expect(mermaid.render).not.toHaveBeenCalled();
});
