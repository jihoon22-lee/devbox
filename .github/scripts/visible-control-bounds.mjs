// Runs read-only on the resolved target. Clip each axis to real ancestor
// scrollports before choosing a pointer location; viewport clipping alone
// can put a tall editor's center in an adjacent pane.
export function visibleControlBounds(bounds) {
  let [left, top, right, bottom] = bounds;
  for (let node = this.parentElement ?? this.getRootNode().host; node; ) {
    const style = getComputedStyle(node);
    const paint = /(?:paint|strict|content)/.test(style.contain);
    const clipX = paint || /^(?:auto|scroll|hidden|clip|overlay)$/.test(style.overflowX);
    const clipY = paint || /^(?:auto|scroll|hidden|clip|overlay)$/.test(style.overflowY);
    if (clipX || clipY) {
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
    node = node.parentElement ?? node.getRootNode().host;
  }
  return [left, top, right, bottom];
}
