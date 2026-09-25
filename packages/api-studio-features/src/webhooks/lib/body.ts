export type BodyEncoding = "utf8" | "base64";

function decodedLength(base64: string): number {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return (base64.length / 4) * 3 - padding;
}

export function bodyPreview(body: string, encoding: BodyEncoding | undefined): string {
  if (encoding === "base64") return `바이너리 본문 · ${decodedLength(body)} bytes`;
  return body.slice(0, 200);
}
