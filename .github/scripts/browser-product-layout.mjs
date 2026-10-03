// Read-only observer, called in an actual browser page by the acceptance runner.
export function observeProductLayout() {
  const rect = (element) => element?.getBoundingClientRect().toJSON() ?? null;
  const shell = document.querySelector(".product-shell");
  const main = shell?.querySelector(":scope > main");
  const controls = main
    ? [...main.querySelectorAll("button,input,select,textarea")].filter(
        (element) =>
          !element.disabled && element.getClientRects().length && getComputedStyle(element).visibility !== "hidden",
      )
    : [];
  const clippedPrimaryControls = controls
    .filter((element) => {
      const r = element.getBoundingClientRect();
      return r.width > 0 && (r.left < 0 || r.right > innerWidth);
    })
    .map((element) => element.getAttribute("aria-label") || element.textContent || element.tagName);
  return {
    viewport: { width: innerWidth, height: innerHeight },
    main: rect(main),
    nav: rect(shell?.querySelector(":scope > aside")),
    notice: rect(shell?.querySelector(".shell-delivery-notice")),
    editor: rect(main?.querySelector(".cm-editor")),
    clippedPrimaryControls,
    horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
  };
}
export function assertProductLayout(result, { editor = false } = {}) {
  if (!result.main || result.main.width < 320 || result.horizontalOverflow || result.clippedPrimaryControls.length)
    throw new Error(`Product layout unavailable: ${JSON.stringify(result)}`);
  if (editor && (!result.editor || result.editor.width < 320))
    throw new Error("Editor narrower than supported task area");
  if (result.notice && result.main.y < result.notice.y + result.notice.height)
    throw new Error("Notice overlaps content");
  return result;
}
