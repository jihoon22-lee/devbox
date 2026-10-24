import { expect, test } from "vitest";
import { sanitizeDiagram } from "./safeSvg";

test("SVG cannot insert resource elements, URLs, navigation, event handlers or CSS imports", () => {
  const safe = sanitizeDiagram(`<svg xmlns="http://www.w3.org/2000/svg">
    <style>@import 'https://example.invalid/css'; text {fill:red}</style>
    <style>text { fill: url(https://example.invalid/paint) }</style>
    <style>text { background-image: image("https://example.invalid/image") }</style>
    <image href="https://example.invalid/x"/><image href="relative.png"/>
    <use href="https://example.invalid/x#i"/><foreignObject><img src="https://example.invalid/y"/></foreignObject>
    <a href="https://example.invalid/page"><text onclick="alert(1)">label</text></a>
    <rect style="fill:url(https://example.invalid/z)" fill="url(https://example.invalid/z)" />
    <animate attributeName="href" to="https://example.invalid/z" />
    <script>alert(1)</script>
  </svg>`);
  const host = document.createElement("div");
  host.append(safe);
  expect(host.querySelector("image,use,foreignObject,a,animate,script,style")).toBeNull();
  expect(host.innerHTML).not.toMatch(/example.invalid|onclick|relative.png/);
  expect(host.textContent?.trim()).toBe("label");
});

test("vector geometry, text, generated styles and local marker references survive", () => {
  const safe = sanitizeDiagram(
    '<svg viewBox="0 0 10 10"><style>text { fill: red; }</style><defs><marker id="m"><path d="M0 0" /></marker></defs><path marker-end="url(#m)" style="stroke: blue" d="M0 0 L10 10"/><text x="1">hello</text></svg>',
  );
  const host = document.createElement("div");
  host.append(safe);
  expect(host.querySelector("svg")).toHaveAttribute("viewBox", "0 0 10 10");
  expect(host.querySelector("path[marker-end]")).toHaveAttribute("marker-end", "url(#m)");
  expect(host.querySelector("style")).toHaveTextContent("fill: red");
  expect(host.querySelector("text")).toHaveTextContent("hello");
});
