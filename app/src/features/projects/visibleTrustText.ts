/** Display escapes only; approval and execution retain the original preview bytes. */
export function visibleTrustText(value: string): string {
  return value.replace(/\\|[\p{Cc}\p{Cf}\u2028\u2029]/gu, (char) => {
    if (char === "\n" || char === "\t") return char;
    if (char === "\\") return "\\\\";
    return char
      .split("")
      .map((unit) => `\\u${unit.charCodeAt(0).toString(16).toUpperCase().padStart(4, "0")}`)
      .join("");
  });
}
