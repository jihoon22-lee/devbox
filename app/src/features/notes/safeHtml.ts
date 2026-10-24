/**
 * A second line of defence for `notes.render` output, which the daemon already sanitizes
 * (ammonia, remote images blocked). The browser and dev gateway have no Tauri CSP, so the
 * preview also refuses anything that could load from the network or run code:
 * only `data:image/*` images stay, active and embedding elements go, and every event
 * handler, inline style and URL-bearing attribute is removed. Links lose `href` too (kept as
 * `data-href`), so no click, middle click, drag or context menu can navigate the app window;
 * the preview decides what a link does.
 * Parsing happens in an inert `<template>`, so nothing loads before this runs.
 */
const DROP = new Set([
  "script",
  "style",
  "iframe",
  "frame",
  "frameset",
  "object",
  "embed",
  "applet",
  "link",
  "meta",
  "base",
  "form",
  "input",
  "button",
  "textarea",
  "select",
  "picture",
  "source",
  "video",
  "audio",
  "track",
  "svg",
  "math",
  "template",
  "noscript",
  "portal",
]);
const URL_ATTRS = new Set([
  "src",
  "srcset",
  "background",
  "poster",
  "formaction",
  "action",
  "ping",
  "xlink:href",
  "lowsrc",
]);
const INLINE_IMAGE = /^data:image\/(png|jpeg|gif|webp|bmp|avif);base64,[a-z0-9+/=\s]*$/i;

function blocked(img: Element): HTMLElement {
  const span = img.ownerDocument.createElement("span");
  span.className = "md-image-missing";
  span.dataset.reason = "remote-blocked";
  const alt = img.getAttribute("alt")?.trim();
  span.textContent = `이미지를 표시할 수 없습니다 (원격 이미지 차단)${alt ? `: ${alt}` : ""}`;
  return span;
}

export function sanitizePreview(html: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = html;
  const all = [...template.content.querySelectorAll("*")];
  for (const element of all) {
    // Children of an element dropped earlier went with it.
    if (!template.content.contains(element)) continue;
    const tag = element.localName;
    if (DROP.has(tag)) {
      element.remove();
      continue;
    }
    if (tag === "img" && !INLINE_IMAGE.test(element.getAttribute("src") ?? "")) {
      element.replaceWith(blocked(element));
      continue;
    }
    for (const attr of [...element.attributes]) {
      const name = attr.name.toLowerCase();
      const keep = name === "src" ? tag === "img" : !(name.startsWith("on") || name === "style" || URL_ATTRS.has(name));
      if (!keep) element.removeAttribute(attr.name);
    }
    if (tag === "a") {
      const href = element.getAttribute("href");
      for (const name of ["href", "target", "download", "data-href", "role", "tabindex"]) element.removeAttribute(name);
      if (href) {
        element.setAttribute("data-href", href);
        element.setAttribute("role", "link");
        element.setAttribute("tabindex", "0");
      }
    }
  }
  return template.content;
}
