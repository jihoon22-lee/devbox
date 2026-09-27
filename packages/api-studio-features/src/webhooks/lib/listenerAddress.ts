export function isLoopbackAddress(address: string | null): boolean {
  if (!address) return false;
  const value = address.trim();
  if (value === "localhost" || value.startsWith("localhost:")) return true;
  if (value.startsWith("[")) {
    const closingBracket = value.indexOf("]");
    return closingBracket > 1 && value.slice(1, closingBracket) === "::1";
  }
  const separator = value.lastIndexOf(":");
  const host = separator >= 0 ? value.slice(0, separator) : value;
  return host === "127.0.0.1" || host === "::1";
}
