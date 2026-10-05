// Observe accessibility/layout via CDP; all renderer actions use real input events.
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { visibleControlBounds } from "./visible-control-bounds.mjs";
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
  async function confirmAction(accept) {
    if (typeof accept !== "boolean") throw new Error("Explicit confirmation decision required");
    const target = { role: "button", name: accept ? "확인" : "취소", scope: { role: "dialog", name: "작업 확인" } };
    await waitForTarget(target);
    await click(target);
  }
  async function click(target, beforePointer) {
    const node = await locate(target);
    if (node.properties?.some((p) => p.name === "disabled" && p.value?.value === true))
      throw new Error("Control disabled");
    if (!node.backendDOMNodeId) throw new Error("Control has no DOM node");
    const params = { backendNodeId: node.backendDOMNodeId };
    await cdp.command("DOM.scrollIntoViewIfNeeded", params);
    const { model } = await cdp.command("DOM.getBoxModel", params);
    const q = model.border;
    if (q.length !== 8 || !q.every(Number.isFinite)) throw new Error("Control layout unavailable");
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
    let left = Math.max(0, Math.min(q[0], q[2], q[4], q[6]));
    let right = Math.min(width, Math.max(q[0], q[2], q[4], q[6]));
    let top = Math.max(0, Math.min(q[1], q[3], q[5], q[7]));
    let bottom = Math.min(height, Math.max(q[1], q[3], q[5], q[7]));
    if (right <= left || bottom <= top) throw new Error("Control center outside viewport");
    const resolved = await cdp.command("DOM.resolveNode", params);
    if (!resolved.object?.objectId) throw new Error("Read-only layout node unavailable");
    try {
      const observed = await cdp.command("Runtime.callFunctionOn", {
        objectId: resolved.object.objectId,
        functionDeclaration: visibleControlBounds.toString(),
        arguments: [{ value: [left, top, right, bottom] }],
        returnByValue: true,
      });
      const bounds = observed.result?.value;
      if (observed.exceptionDetails || !Array.isArray(bounds) || bounds.length !== 4 || !bounds.every(Number.isFinite))
        throw new Error("Control clipping layout unavailable");
      [left, top, right, bottom] = bounds;
    } finally {
      await cdp.command("Runtime.releaseObject", { objectId: resolved.object.objectId });
    }
    if (right <= left || bottom <= top) throw new Error("Control is clipped outside its scrollport");
    // Hit ownership still rejects unrelated overlays and transformed corners.
    const x = (left + right) / 2;
    const y = (top + bottom) / 2;
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
    beforePointer?.();
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
    const code = /^[a-z]$/i.test(key) ? `Key${key.toUpperCase()}` : /^\d$/.test(key) ? `Digit${key}` : key;
    const params = { key: key === "Space" ? " " : key, code, windowsVirtualKeyCode: virtual, modifiers };
    // Chromium needs character text for native keypress/default activation.
    // Ctrl/Alt/Meta shortcuts must not insert printable text into the editor.
    const text =
      modifiers & 11 ? undefined : key === "Enter" ? "\r" : key === "Space" ? " " : key.length === 1 ? key : undefined;
    await cdp.command("Input.dispatchKeyEvent", {
      type: "keyDown",
      ...params,
      ...(text === undefined ? {} : { text }),
    });
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
      if (node.role?.value === "textbox" && node.backendDOMNodeId) {
        const resolved = await cdp.command("DOM.resolveNode", { backendNodeId: node.backendDOMNodeId });
        const objectId = resolved.object?.objectId;
        if (!objectId) throw new Error("Read-only textbox node unavailable");
        let readFailed = false;
        try {
          const result = await cdp.command("Runtime.callFunctionOn", {
            objectId,
            functionDeclaration:
              "function() { return this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement ? this.value : null; }",
            returnByValue: true,
          });
          if (result.exceptionDetails) throw new Error("Read-only textbox value unavailable");
          if (typeof result.result?.value === "string") return result.result.value;
        } catch (error) {
          readFailed = true;
          throw error;
        } finally {
          try {
            await cdp.command("Runtime.releaseObject", { objectId });
          } catch (error) {
            if (!readFailed) throw error;
          }
        }
        if (typeof node.value?.value !== "string") throw new Error("Textbox value absent from accessibility tree");
      }
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
    confirmAction,
    async clickWithConfirmation(target, accept) {
      if (typeof accept !== "boolean") throw new Error("Explicit confirmation decision required");
      await click(target);
      await confirmAction(accept);
    },
    async clickWithDialog(target, accept) {
      if (typeof accept !== "boolean") throw new Error("Explicit dialog decision required");
      if (typeof cdp.onEvent !== "function") throw new Error("Dialog opening observation required");
      let handled = false,
        timer,
        unsubscribe,
        resolveDecision,
        rejectDecision,
        firstFailure;
      const decision = new Promise((resolve, reject) => {
        resolveDecision = resolve;
        rejectDecision = (error) => {
          firstFailure ??= error;
          reject(error);
        };
      });
      const arm = () => {
        timer = setTimeout(() => {
          handled = true;
          rejectDecision(new Error("Expected confirmation dialog did not open"));
        }, 10000);
        unsubscribe = cdp.onEvent("Page.javascriptDialogOpening", ({ type }) => {
          if (handled) return;
          clearTimeout(timer);
          handled = true;
          if (type !== "confirm") {
            rejectDecision(new Error("Expected confirmation dialog type"));
            return;
          }
          cdp.command("Page.handleJavaScriptDialog", { accept }).then(resolveDecision, rejectDecision);
        });
      };
      try {
        // Resolve geometry first. Keep observation until both bounded protocol
        // operations settle, including invalid dialogs and pointer errors.
        const pointer = click(target, arm).catch((error) => {
          rejectDecision(error);
          throw error;
        });
        const outcomes = await Promise.allSettled([pointer, decision]);
        const failure = outcomes.find((outcome) => outcome.status === "rejected");
        if (failure) throw firstFailure ?? failure.reason;
      } finally {
        clearTimeout(timer);
        unsubscribe?.();
      }
    },
    async confirmDialog(accept) {
      if (typeof accept !== "boolean") throw new Error("Explicit dialog decision required");
      await cdp.command("Page.handleJavaScriptDialog", { accept });
    },
    async pressWithPrompt(key, accept, promptText) {
      if (typeof accept !== "boolean" || (accept && typeof promptText !== "string"))
        throw new Error("Explicit prompt decision required");
      if (typeof cdp.onEvent !== "function") throw new Error("Dialog opening observation required");
      let unsubscribe,
        timer,
        rejectDecision,
        firstFailure,
        handled = false;
      const decision = new Promise((resolve, reject) => {
        rejectDecision = (error) => {
          firstFailure ??= error;
          reject(error);
        };
        timer = setTimeout(() => {
          handled = true;
          rejectDecision(new Error("Expected prompt did not open"));
        }, 10000);
        unsubscribe = cdp.onEvent("Page.javascriptDialogOpening", ({ type }) => {
          if (handled) return;
          handled = true;
          clearTimeout(timer);
          if (type !== "prompt") {
            rejectDecision(new Error("Expected prompt dialog type"));
            return;
          }
          cdp
            .command("Page.handleJavaScriptDialog", { accept, ...(accept ? { promptText } : {}) })
            .then(resolve, rejectDecision);
        });
      });
      try {
        const input = press(key).catch((error) => {
          rejectDecision(error);
          throw error;
        });
        const outcomes = await Promise.allSettled([input, decision]);
        const failure = outcomes.find((outcome) => outcome.status === "rejected");
        if (failure) throw firstFailure ?? failure.reason;
      } finally {
        clearTimeout(timer);
        unsubscribe?.();
      }
    },
    closeOwnedWindow,
  };
}
