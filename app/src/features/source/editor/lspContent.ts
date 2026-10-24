import type { LSPClientConfig } from "@codemirror/lsp-client";
import { EditorView, ViewPlugin } from "@codemirror/view";
import { openExternal } from "../../../shell/external";
import { sanitizePreview } from "../../notes/safeHtml";

const TAGS = new Set(
  "a p br strong em b i code pre blockquote ul ol li hr h1 h2 h3 h4 h5 h6 table thead tbody tr th td del span div".split(
    " ",
  ),
);
const web = (href: string) => {
  try {
    return ["http:", "https:"].includes(new URL(href).protocol);
  } catch {
    return false;
  }
};
export function sanitizeLspHTML(html: string): string {
  const fragment = sanitizePreview(html);
  for (const el of [...fragment.querySelectorAll("*")]) {
    if (!fragment.contains(el)) continue;
    if (!TAGS.has(el.localName)) {
      el.remove();
      continue;
    }
    const href = el.localName === "a" ? el.getAttribute("data-href") : null;
    for (const attr of [...el.attributes]) el.removeAttribute(attr.name);
    if (href && web(href)) {
      el.setAttribute("data-href", href);
      el.setAttribute("role", "link");
      el.setAttribute("tabindex", "0");
    }
  }
  const host = document.createElement("div");
  host.append(fragment);
  return host.innerHTML;
}

const links = ViewPlugin.fromClass(
  class {
    constructor(private view: EditorView) {
      view.dom.addEventListener("click", this.follow);
      view.dom.addEventListener("keydown", this.follow);
    }
    follow = (event: Event) => {
      if (event instanceof KeyboardEvent && event.key !== "Enter") return;
      const link = (event.target as Element).closest?.(".cm-lsp-documentation a[data-href]");
      if (!link || !this.view.dom.contains(link)) return;
      event.preventDefault();
      const href = link.getAttribute("data-href")!;
      if (web(href)) void openExternal(href);
    };
    destroy() {
      this.view.dom.removeEventListener("click", this.follow);
      this.view.dom.removeEventListener("keydown", this.follow);
    }
  },
);

/** All installed 6.3.0 documentation sinks call docToHTML and this sanitizer. */
export function contentConfig(showMessage: (text: string) => void = () => {}): LSPClientConfig {
  return {
    sanitizeHTML: sanitizeLspHTML,
    extensions: [links],
    notificationHandlers: {
      "window/logMessage": () => true,
      "window/showMessage": (_client, params) => {
        if (typeof params?.message === "string") showMessage(params.message);
        return true;
      },
    },
  };
}
