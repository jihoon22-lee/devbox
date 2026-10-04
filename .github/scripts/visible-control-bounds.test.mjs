import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { visibleControlBounds } from "./visible-control-bounds.mjs";
function ancestor({
  x = 20,
  y = 300,
  width = 480,
  height = 70,
  overflowX = "auto",
  overflowY = "auto",
  parent = null,
} = {}) {
  return {
    style: { overflowX, overflowY, contain: "none" },
    clientLeft: 2,
    clientTop: 2,
    clientWidth: width - 4,
    clientHeight: height - 4,
    offsetWidth: width,
    offsetHeight: height,
    getBoundingClientRect: () => ({ left: x, top: y, width, height }),
    parentElement: parent,
    getRootNode: () => ({}),
  };
}
function observe(parent, bounds = [0, 0, 720, 480]) {
  return Array.from(
    runInNewContext(`(${visibleControlBounds}).call(target, bounds)`, {
      target: { parentElement: parent },
      bounds,
      getComputedStyle: (node) => node.style,
    }),
  );
}
test("a tall editor clips to its ancestor scrollport rather than the adjacent pane", () => {
  assert.deepEqual(observe(ancestor()), [22, 302, 498, 368]);
});
test("axis-specific nested clipping retains only the common visible rectangle", () => {
  const outer = ancestor({ x: 100, y: 0, width: 300, height: 480, overflowY: "visible" });
  assert.deepEqual(observe(ancestor({ parent: outer, overflowX: "visible" })), [102, 302, 398, 368]);
});
test("unclipped ancestors do not restrict the viewport and fully clipped targets stay empty", () => {
  assert.deepEqual(observe(ancestor({ overflowX: "visible", overflowY: "visible" })), [0, 0, 720, 480]);
  const [left, top, right, bottom] = observe(ancestor({ y: 600 }));
  assert.ok(left < right && top >= bottom);
});
