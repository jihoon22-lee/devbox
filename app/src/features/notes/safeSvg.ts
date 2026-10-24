/** SVG is parsed inertly and filtered before insertion. Render-time resources are separately
 * excluded by mermaidPolicy: this is defence against resources in the returned SVG. */
const TAGS = new Set([
  "svg",
  "g",
  "path",
  "rect",
  "line",
  "polygon",
  "polyline",
  "circle",
  "ellipse",
  "text",
  "tspan",
  "defs",
  "marker",
  "title",
  "desc",
  "clippath",
  "style",
]);
const ATTRS = new Set([
  "id",
  "class",
  "role",
  "aria-label",
  "aria-labelledby",
  "aria-describedby",
  "aria-roledescription",
  "xmlns",
  "viewbox",
  "width",
  "height",
  "x",
  "y",
  "x1",
  "x2",
  "y1",
  "y2",
  "cx",
  "cy",
  "r",
  "rx",
  "ry",
  "d",
  "points",
  "transform",
  "dx",
  "dy",
  "text-anchor",
  "dominant-baseline",
  "fill",
  "fill-opacity",
  "fill-rule",
  "stroke",
  "stroke-width",
  "stroke-opacity",
  "stroke-dasharray",
  "stroke-dashoffset",
  "stroke-linecap",
  "stroke-linejoin",
  "opacity",
  "font-size",
  "font-family",
  "font-weight",
  "font-style",
  "marker-start",
  "marker-mid",
  "marker-end",
  "markerwidth",
  "markerheight",
  "markerunits",
  "orient",
  "refx",
  "refy",
  "preserveaspectratio",
  "clip-path",
  "style",
]);

// CSS escapes, comments and at-rules can conceal URLs/imports. Generated Mermaid CSS needs
// none of them. Only simple same-document fragment URLs (arrow/clip definitions) are retained.
function localCss(value: string): boolean {
  const withoutFragments = value.replace(/url\(\s*#[a-zA-Z0-9_-]+\s*\)/g, "");
  if (/@|\\|\/\*|[<>]/.test(withoutFragments)) return false;
  // A function allowlist also excludes image()/image-set()/src() and future URL forms.
  return [...withoutFragments.matchAll(/([a-z-]+)\s*\(/gi)].every((match) =>
    /^(?:rgb|rgba|hsl|hsla|calc|var|matrix|translate|scale|rotate|skew|not|is|where|nth-child)$/i.test(match[1] ?? ""),
  );
}

export function sanitizeDiagram(svg: string, xml = false): DocumentFragment {
  const template = document.createElement("template");
  if (xml) {
    // Source SVG is XML. HTML parsing of foreignObject/iframe can swallow safe siblings.
    if (/<!DOCTYPE|<!ENTITY/i.test(svg)) return template.content;
    const parsed = new DOMParser().parseFromString(svg, "image/svg+xml");
    if (parsed.querySelector("parsererror") || parsed.documentElement.localName !== "svg") return template.content;
    template.content.append(document.importNode(parsed.documentElement, true));
  } else template.innerHTML = svg;
  for (const element of [...template.content.querySelectorAll("*")]) {
    if (!template.content.contains(element)) continue;
    const tag = element.localName.toLowerCase();
    if (tag === "a") {
      element.replaceWith(...element.childNodes);
      continue;
    }
    if (element.namespaceURI !== "http://www.w3.org/2000/svg" || !TAGS.has(tag)) {
      element.remove();
      continue;
    }
    if (tag === "style" && !localCss(element.textContent ?? "")) {
      element.remove();
      continue;
    }
    for (const attr of [...element.attributes]) {
      if (!ATTRS.has(attr.name.toLowerCase()) || !localCss(attr.value)) element.removeAttribute(attr.name);
    }
  }
  return template.content;
}
