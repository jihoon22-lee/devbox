const normalizedHost = (host: string) => host.toLowerCase().replace(/\.$/, "");

export function parseOsc7(data: string, localHostname?: string | null): string | null {
  try {
    const url = new URL(data);
    if (url.protocol !== "file:" || url.search || url.hash) return null;
    const authority = normalizedHost(url.hostname);
    if (authority && authority !== "localhost" && (!localHostname || authority !== normalizedHost(localHostname)))
      return null;
    const path = decodeURIComponent(url.pathname);
    return path.startsWith("/") && !/[\u0000-\u001f\u007f]/.test(path) ? path : null;
  } catch {
    return null;
  }
}
