export function parseOsc7(data: string): string | null {
  try {
    const url = new URL(data);
    if (url.protocol !== "file:" || url.search || url.hash) return null;
    const path = decodeURIComponent(url.pathname);
    return path.startsWith("/") && !/[\u0000-\u001f\u007f]/.test(path) ? path : null;
  } catch {
    return null;
  }
}
