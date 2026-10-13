/** Task 22 will add the Windows commands; until then all hosts use this title fallback. */
export function nativeBadge(attention: number, _open: number) {
  const title = attention ? `(${attention}) devbox` : "devbox";
  if (document.title !== title) document.title = title;
}
