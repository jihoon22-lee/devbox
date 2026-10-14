/** Same accepted transports and secret restrictions as projects.clone. */
export function repositoryName(url: string): string | undefined {
  if (!url || url.startsWith("-") || /[\x00-\x1f\x7f]/.test(url)) return;
  const separator = url.indexOf("://");
  if (separator >= 0) {
    const scheme = url.slice(0, separator);
    if (!["http", "https", "ssh", "git", "file"].includes(scheme)) return;
    const authority = url.slice(separator + 3).split("/")[0] ?? "";
    if (scheme !== "file" && !authority) return;
    if (authority.includes("@")) {
      const user = authority.slice(0, authority.lastIndexOf("@"));
      if (scheme !== "ssh" || !user || /[:%]/.test(user)) return;
    }
  } else if (url.includes("::") || (!url.startsWith("/") && !/^[^/:]+:[^:]+$/.test(url))) return;
  const last = url
    .replace(/\/+$/, "")
    .split(/[/:]/)
    .at(-1)
    ?.replace(/\.git$/, "");
  if (!last || last === "." || last === ".." || /[\\?#]/.test(last)) return;
  return last;
}
