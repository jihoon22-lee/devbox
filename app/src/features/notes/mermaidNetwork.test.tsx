import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { Preview } from "./Preview";
import { NotesWire } from "./test-wire";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test.each([
  'flowchart TD\nA@{ img: "https://example.invalid/tracker.png", label: "image", h: 60 }',
  'sequenceDiagram\nparticipant A\nproperties A: {"icon":"https://example.invalid/tracker.png"}\nA->>A: hi',
  'sequenceDiagram\nparticipant A\nproperties A: {"icon":"tracker.png"}\nA->>A: hi',
])("real Mermaid cannot load or insert resource images: %s", async (source) => {
  const urls: string[] = [];
  class TrackedImage {
    naturalWidth = 10;
    naturalHeight = 10;
    set src(url: string) {
      urls.push(url);
    }
    async decode() {}
  }
  vi.stubGlobal("Image", TrackedImage);
  const wire = new NotesWire();
  wire.handlers["notes.render"] = () => ({
    html: '<div class="mermaid-block" data-idx="0"></div>',
    mermaid: [source],
  });
  render(
    <RpcProvider client={wire.client}>
      <Preview path="a.md" text="diagram" onOpenNote={() => {}} />
    </RpcProvider>,
  );
  await screen.findByText("외부 리소스 차단을 위해 이 다이어그램 종류나 문법은 표시하지 않습니다.");
  expect(urls).toEqual([]);
});

test.each(["flowchart TD; A-->B", "sequenceDiagram\nparticipant A\nA->>A: hi"])(
  "ordinary vector Mermaid still renders: %s",
  async (source) => {
    const original = Object.getOwnPropertyDescriptor(SVGElement.prototype, "getBBox");
    const textLength = Object.getOwnPropertyDescriptor(SVGElement.prototype, "getComputedTextLength");
    Object.defineProperty(SVGElement.prototype, "getComputedTextLength", { configurable: true, value: () => 10 });
    Object.defineProperty(SVGElement.prototype, "getBBox", {
      configurable: true,
      value: () => ({ x: 0, y: 0, width: 10, height: 10 }),
    });
    try {
      const wire = new NotesWire();
      wire.handlers["notes.render"] = () => ({
        html: '<div class="mermaid-block" data-idx="0"></div>',
        mermaid: [source],
      });
      const view = render(
        <RpcProvider client={wire.client}>
          <Preview path="a.md" text="diagram" onOpenNote={() => {}} />
        </RpcProvider>,
      );
      await waitFor(() => expect(view.container.querySelector("svg")).not.toBeNull());
    } finally {
      if (textLength) Object.defineProperty(SVGElement.prototype, "getComputedTextLength", textLength);
      else Reflect.deleteProperty(SVGElement.prototype, "getComputedTextLength");
      if (original) Object.defineProperty(SVGElement.prototype, "getBBox", original);
      else Reflect.deleteProperty(SVGElement.prototype, "getBBox");
    }
  },
);
