// Observe accessibility/layout via CDP; all renderer actions use real input events.
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
export function createUiDriver({ cdp, evidenceRoot, closeOwnedWindow }) {
  if (typeof closeOwnedWindow !== "function") throw new Error("Owned native close adapter required");
  async function locate({ role, name, scope }, allowAbsent = false) {
    if (!role || typeof name !== "string") throw new Error("Exact accessible target required");
    const { nodes } = await cdp.command("Accessibility.getFullAXTree");
    let candidates = nodes;
    if (scope) {
      if (!scope.role || typeof scope.name !== "string") throw new Error("Exact accessible scope required");
      const ancestors = nodes.filter(
        (node) => !node.ignored && node.role?.value === scope.role && node.name?.value === scope.name,
      );
      if (allowAbsent && ancestors.length === 0) return null;
      if (ancestors.length !== 1) throw new Error("Accessible scope must be unique");
      const descendants = new Set();
      const queue = [...(ancestors[0].childIds ?? [])];
      const byId = new Map(nodes.map((node) => [node.nodeId, node]));
      while (queue.length) {
        const id = queue.pop();
        if (descendants.has(id)) continue;
        descendants.add(id);
        queue.push(...(byId.get(id)?.childIds ?? []));
      }
      candidates = nodes.filter((node) => descendants.has(node.nodeId));
    }
    const found = candidates.filter((n) => !n.ignored && n.role?.value === role && n.name?.value === name);
    if (allowAbsent && found.length === 0) return null;
    if (found.length !== 1) throw new Error(`Expected one accessible ${role}: ${name}; found ${found.length}`);
    return { ...found[0], axNodes: nodes };
  }
  async function waitForTarget(target, { timeoutMs = 10000 } = {}) {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 30000)
      throw new Error("Bounded target readiness timeout required");
    const deadline = performance.now() + timeoutMs;
    do {
      const node = await locate(target, true);
      if (
        node?.backendDOMNodeId &&
        !node.properties?.some((property) => property.name === "disabled" && property.value?.value === true)
      )
        return;
      const remaining = deadline - performance.now();
      if (remaining <= 0) break;
      // Only read accessibility state again; never retry an input action.
      await delay(Math.min(100, remaining));
    } while (performance.now() < deadline);
    throw new Error(`Timed out waiting for accessible ${target.role}: ${target.name}`);
  }
  async function click(target) {
    const node = await locate(target);
    if (node.properties?.some((p) => p.name === "disabled" && p.value?.value === true))
      throw new Error("Control disabled");
    if (!node.backendDOMNodeId) throw new Error("Control has no DOM node");
    const params = { backendNodeId: node.backendDOMNodeId };
    await cdp.command("DOM.scrollIntoViewIfNeeded", params);
    const { model } = await cdp.command("DOM.getBoxModel", params);
    const q = model.border;
    const x = (q[0] + q[2] + q[4] + q[6]) / 4;
    const y = (q[1] + q[3] + q[5] + q[7]) / 4;
    if (!Number.isFinite(x) || !Number.isFinite(y)) throw new Error("Control layout unavailable");
    // Box quads and Input use viewport CSS pixels. DOM hit testing uses page
    // CSS pixels, including the root scroll offset after scrollIntoView.
    const { cssLayoutViewport } = await cdp.command("Page.getLayoutMetrics");
    const pageX = cssLayoutViewport?.pageX;
    const pageY = cssLayoutViewport?.pageY;
    if (!Number.isFinite(pageX) || !Number.isFinite(pageY)) throw new Error("Page layout unavailable");
    const width = cssLayoutViewport.clientWidth;
    const height = cssLayoutViewport.clientHeight;
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0)
      throw new Error("Page viewport unavailable");
    if (x < 0 || y < 0 || x >= width || y >= height) throw new Error("Control center outside viewport");
    const hit = await cdp.command("DOM.getNodeForLocation", {
      x: Math.round(x + pageX),
      y: Math.round(y + pageY),
      includeUserAgentShadowDOM: true,
    });
    if (!Number.isInteger(hit.backendNodeId) || hit.backendNodeId <= 0) throw new Error("No visible hit target");
    if (hit.backendNodeId !== node.backendDOMNodeId) {
      const handles = [];
      try {
        async function resolve(backendNodeId) {
          const result = await cdp.command("DOM.resolveNode", { backendNodeId });
          if (!result.object?.objectId) throw new Error("Read-only hit node unavailable");
          handles.push(result.object.objectId);
          return result.object.objectId;
        }
        const targetObject = await resolve(node.backendDOMNodeId),
          hitObject = await resolve(hit.backendNodeId);
        const result = await cdp.command("Runtime.callFunctionOn", {
          objectId: targetObject,
          functionDeclaration:
            "function(hit) { for(let node=hit;node;) { if(this===node || this.contains(node)) return true; const root=node.getRootNode(); node=root.host ?? null; } return false; }",
          arguments: [{ objectId: hitObject }],
          returnByValue: true,
        });
        if (result.exceptionDetails || result.result?.value !== true)
          throw new Error("Accessible target is covered by another element");
      } finally {
        const released = await Promise.allSettled(
          handles.map((objectId) => cdp.command("Runtime.releaseObject", { objectId })),
        );
        if (released.some((result) => result.status === "rejected"))
          throw new Error("Read-only hit handles could not be released");
      }
    }
    await cdp.command("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
    await cdp.command("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  }
  async function press(keys) {
    const parts = keys.split("+");
    const key = parts.pop();
    let modifiers = 0;
    for (const part of parts) {
      const mask = { Alt: 1, Control: 2, Ctrl: 2, Meta: 4, Shift: 8 }[part];
      if (!mask) throw new Error("Unsupported modifier");
      modifiers |= mask;
    }
    const map = {
      Enter: 13,
      Escape: 27,
      Tab: 9,
      Backspace: 8,
      Delete: 46,
      ArrowLeft: 37,
      ArrowUp: 38,
      ArrowRight: 39,
      ArrowDown: 40,
      Home: 36,
      End: 35,
      Space: 32,
    };
    const virtual = map[key] ?? (key.length === 1 ? key.toUpperCase().charCodeAt(0) : null);
    if (!virtual) throw new Error("Unsupported key");
    const params = { key: key === "Space" ? " " : key, windowsVirtualKeyCode: virtual, modifiers };
    await cdp.command("Input.dispatchKeyEvent", { type: "keyDown", ...params });
    await cdp.command("Input.dispatchKeyEvent", { type: "keyUp", ...params });
  }
  return {
    click,
    waitForTarget,
    press,
    async fill(target, text) {
      await click(target);
      await press("Control+a");
      await cdp.command("Input.insertText", { text });
    },
    async text(target) {
      const node = await locate(target);
      if (["alert", "status", "region", "dialog"].includes(node.role?.value)) {
        const byId = new Map(node.axNodes.map((item) => [item.nodeId, item]));
        const text = [],
          visited = new Set();
        function collect(item) {
          if (!item || visited.has(item.nodeId)) return;
          visited.add(item.nodeId);
          if (item.role?.value === "StaticText") text.push(String(item.name?.value ?? ""));
          else for (const id of item.childIds ?? []) collect(byId.get(id));
        }
        collect(node);
        return text.join(" ");
      }
      return String(node.value?.value ?? node.name?.value ?? "");
    },
    async screenshot(name) {
      if (!/^[a-zA-Z0-9_-]+$/.test(name)) throw new Error("Invalid evidence name");
      await mkdir(evidenceRoot, { recursive: true });
      const { data } = await cdp.command("Page.captureScreenshot", { format: "png" });
      const file = path.resolve(evidenceRoot, `${name}.png`);
      await writeFile(file, Buffer.from(data, "base64"));
      return file;
    },
    async typeText(text) {
      if (typeof text !== "string") throw new Error("Text input required");
      await cdp.command("Input.insertText", { text });
    },
    async confirmDialog(accept) {
      if (typeof accept !== "boolean") throw new Error("Explicit dialog decision required");
      await cdp.command("Page.handleJavaScriptDialog", { accept });
    },
    closeOwnedWindow,
  };
}
