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
  position = "static",
  transform = "none",
  filter = "none",
  contain = "none",
  parent = null,
} = {}) {
  return {
    style: { overflowX, overflowY, contain, position, transform, filter },
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
      target: { parentElement: parent, style: { position: "static" } },
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
test("viewport-fixed dialog escapes its source pane but retains its own scrollport", () => {
  const pane = ancestor({ y: 435, height: 45 });
  const backdrop = ancestor({ position: "fixed", overflowX: "visible", overflowY: "visible", parent: pane });
  const dialog = ancestor({ x: 24, y: 128, width: 672, height: 223, parent: backdrop });
  assert.deepEqual(observe(dialog, [110, 293, 231, 326]), [110, 293, 231, 326]);
  assert.deepEqual(observe(dialog, [110, 293, 231, 400]), [110, 293, 231, 349]);
});
test("fixed descendants remain clipped by their transform filter or containment block", () => {
  for (const effect of [{ transform: "matrix(1,0,0,1,0,0)" }, { filter: "blur(0px)" }, { contain: "layout" }]) {
    const pane = ancestor({ y: 435, height: 45, ...effect });
    const backdrop = ancestor({ position: "fixed", overflowX: "visible", overflowY: "visible", parent: pane });
    const [, top, , bottom] = observe(backdrop, [110, 293, 231, 326]);
    assert.ok(top >= bottom);
  }
});
