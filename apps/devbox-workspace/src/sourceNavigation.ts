/** Git output is display data. Files still performs native root/object admission. */
export function sourceFilePath(root: string, relative: string): string | null {
  if (!relative || relative.length > 32768 || /[\\:\x00-\x1f\x7f]/.test(relative)) return null;
  const parts = relative.split("/");
  if (parts.some(part => !part || part === "." || part === "..")) return null;
  return `${root.replace(/[\\/]+$/, "")}/${relative}`;
}
