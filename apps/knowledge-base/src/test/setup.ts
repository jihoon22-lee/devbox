// vitest + Testing Library용 jest-dom 매처(toBeInTheDocument 등)를 expect에 등록한다.
import "@testing-library/jest-dom/vitest";

// CodeMirror measures text ranges in animation-frame callbacks. jsdom does
// not implement these layout APIs, so provide deterministic empty geometry
// and prevent a completed editor test from leaking an uncaught measurement
// error into the suite.
if (!("getClientRects" in Range.prototype)) {
  Object.defineProperty(Range.prototype, "getClientRects", {
    configurable: true,
    value: () => ({
      length: 0,
      item: () => null,
      *[Symbol.iterator]() { /* empty */ },
    }),
  });
}
