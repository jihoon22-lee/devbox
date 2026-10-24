import { md5 } from "./md5";
export async function hash(data: string, algorithm: string): Promise<string> {
  const bytes = new TextEncoder().encode(data);
  if (bytes.length > 1024 * 1024) throw new Error("입력은 1MiB까지 처리할 수 있습니다.");
  if (algorithm === "md5") return md5(data);
  const digest = algorithm === "sha256" ? "SHA-256" : algorithm === "sha512" ? "SHA-512" : undefined;
  if (!digest) throw new Error("지원하지 않는 해시 알고리즘입니다.");
  const result = await crypto.subtle.digest(digest, bytes);
  return Array.from(new Uint8Array(result), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
