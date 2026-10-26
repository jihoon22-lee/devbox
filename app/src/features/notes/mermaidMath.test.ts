import mermaid from "mermaid";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { networkFreeDiagram } from "./mermaidPolicy";
import { sanitizeDiagram } from "./safeSvg";

// Geometry and MathMLElement capability shims exercise real KaTeX semantic output in jsdom.
// They do not prove browser layout or visible math: the application strips foreignObject.
let restoreGeometry: () => void;
let renderId = 0;
const fetchAttempt = vi.fn();
const imageUrls: string[] = [];
let xhrAttempt: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  const names = ["getBBox", "getComputedTextLength"] as const;
  const descriptors = names.map((name) => Object.getOwnPropertyDescriptor(SVGElement.prototype, name));
  Object.defineProperty(SVGElement.prototype, "getBBox", {
    configurable: true,
    value: () => ({ x: 0, y: 0, width: 10, height: 10 }),
  });
  Object.defineProperty(SVGElement.prototype, "getComputedTextLength", { configurable: true, value: () => 10 });
  restoreGeometry = () => {
    names.forEach((name, index) => {
      const descriptor = descriptors[index];
      if (descriptor) Object.defineProperty(SVGElement.prototype, name, descriptor);
      else Reflect.deleteProperty(SVGElement.prototype, name);
    });
  };
  vi.stubGlobal("MathMLElement", Element);
  imageUrls.length = 0;
  class TrackedImage {
    set src(url: string) {
      imageUrls.push(url);
    }
  }
  vi.stubGlobal("Image", TrackedImage);
  fetchAttempt.mockReset();
  vi.stubGlobal("fetch", fetchAttempt.mockRejectedValue(new Error("unexpected resource request")));
  xhrAttempt = vi.spyOn(XMLHttpRequest.prototype, "open").mockImplementation(() => {
    throw new Error("unexpected XHR");
  });
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    layout: "dagre",
    theme: "default",
    look: "classic",
  });
});
afterEach(() => {
  expect(imageUrls).toEqual([]);
  expect(fetchAttempt).not.toHaveBeenCalled();
  expect(xhrAttempt).not.toHaveBeenCalled();
  vi.restoreAllMocks();
  restoreGeometry();
  vi.unstubAllGlobals();
});

async function output(source: string) {
  expect(networkFreeDiagram(source)).toBe(true);
  const { svg } = await mermaid.render(`math-${++renderId}`, source);
  const raw = document.createElement("template");
  raw.innerHTML = svg;
  const safe = document.createElement("div");
  safe.append(sanitizeDiagram(svg));
  return { raw: raw.content, safe };
}

test("real sequence KaTeX produces MathML, but the preview removes its unsupported foreignObject", async () => {
  const { raw, safe } = await output("sequenceDiagram\nparticipant A\nA->>A: $$x^2+1$$");
  const math = raw.querySelector("foreignObject math");
  expect(math).not.toBeNull();
  expect(math?.querySelector("msup mi")?.textContent).toBe("x");
  expect(math?.querySelector("msup mn")?.textContent).toBe("2");
  expect(math?.querySelector("mo")?.textContent).toBe("+");
  expect([...(math?.querySelectorAll("mn") ?? [])].map((el) => el.textContent)).toEqual(["2", "1"]);
  expect(math?.textContent).not.toMatch(/\$\$|not supported|not available/i);
  expect(safe.querySelector("foreignObject, math, msup")).toBeNull();
  expect([...safe.querySelectorAll("text,tspan")].map((el) => el.textContent).join(" ")).not.toContain("x");
  expect(safe.querySelector("line,path,rect")).not.toBeNull();
});

test("flowchart htmlLabels=false preserves literal dollar text rather than claiming typeset math", async () => {
  const { raw, safe } = await output('flowchart TD; A["$$x^2+1$$"]');
  expect(raw.querySelector("math,foreignObject")).toBeNull();
  expect([...raw.querySelectorAll("text")].map((el) => el.textContent).join(" ")).toContain("$$x^2+1$$");
  expect([...safe.querySelectorAll("text")].map((el) => el.textContent).join(" ")).toContain("$$x^2+1$$");
});

test("LaTeX command inputs remain blocked before the renderer", () => {
  for (const formula of [
    String.raw`\frac{1}{2}`,
    String.raw`\href{https://example.invalid}{x}`,
    String.raw`\includegraphics{https://example.invalid/x}`,
  ]) {
    expect(networkFreeDiagram(`sequenceDiagram\nA->>A: $$${formula}$$`)).toBe(false);
  }
});
