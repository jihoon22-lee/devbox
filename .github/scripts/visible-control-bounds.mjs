// Runs read-only on the resolved target. Clip each axis to real ancestor
// scrollports before choosing a pointer location; viewport clipping alone
// can put a tall editor's center in an adjacent pane.
export function visibleControlBounds(bounds) {
  let [left, top, right, bottom] = bounds;
  let fixed = getComputedStyle(this).position === "fixed";
  for (let node = this.parentElement ?? this.getRootNode().host; node; ) {
    const style = getComputedStyle(node);
    // A fixed dialog escapes ordinary source-pane overflow. Resume clipping at
    // its actual containing block, retaining scrollports inside the dialog.
    const fixedContainer =
      ["transform", "translate", "rotate", "scale", "perspective", "filter", "backdropFilter"].some(
        (property) => style[property] && style[property] !== "none",
      ) ||
      /(?:layout|paint|strict|content)/.test(style.contain) ||
      /(?:transform|translate|rotate|scale|perspective|filter|backdrop-filter|contain)/.test(style.willChange) ||
      style.contentVisibility === "auto";
    if (fixed && fixedContainer) fixed = false;
    const paint = /(?:paint|strict|content)/.test(style.contain);
    const clipX = paint || /^(?:auto|scroll|hidden|clip|overlay)$/.test(style.overflowX);
    const clipY = paint || /^(?:auto|scroll|hidden|clip|overlay)$/.test(style.overflowY);
    if (!fixed && (clipX || clipY)) {
      const rect = node.getBoundingClientRect();
      const scaleX = node.offsetWidth ? rect.width / node.offsetWidth : 1;
      const scaleY = node.offsetHeight ? rect.height / node.offsetHeight : 1;
      const x = rect.left + node.clientLeft * scaleX;
      const y = rect.top + node.clientTop * scaleY;
      if (clipX) {
        left = Math.max(left, x);
        right = Math.min(right, x + node.clientWidth * scaleX);
      }
      if (clipY) {
        top = Math.max(top, y);
        bottom = Math.min(bottom, y + node.clientHeight * scaleY);
      }
    }
    if (style.position === "fixed") fixed = true;
    node = node.parentElement ?? node.getRootNode().host;
  }
  return [left, top, right, bottom];
}
