// Observe accessibility/layout via CDP; all renderer actions use real input events.
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
export function createUiDriver({ cdp, evidenceRoot, closeOwnedWindow }) {
  if (typeof closeOwnedWindow !== "function") throw new Error("Owned native close adapter required");
  async function locate({ role, name }) {
    if (!role || typeof name !== "string") throw new Error("Exact accessible target required");
    const { nodes } = await cdp.command("Accessibility.getFullAXTree");
    const found = nodes.filter((n) => !n.ignored && n.role?.value === role && n.name?.value === name);
    if (found.length !== 1) throw new Error(`Expected one accessible ${role}: ${name}; found ${found.length}`);
    return found[0];
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
    const hit = await cdp.command("DOM.getNodeForLocation", {
      x: Math.round(x),
      y: Math.round(y),
      includeUserAgentShadowDOM: true,
    });
    // Input still uses hit testing; evidence assertions must confirm resulting state.
    if (hit.backendNodeId === 0) throw new Error("No visible hit target");
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
    press,
    async fill(target, text) {
      await click(target);
      await press("Control+a");
      await cdp.command("Input.insertText", { text });
    },
    async text(target) {
      const node = await locate(target);
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
    closeOwnedWindow,
  };
}
